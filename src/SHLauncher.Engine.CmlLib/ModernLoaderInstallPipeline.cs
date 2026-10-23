using System.Text.Json;
using CmlLib.Core;
using CmlLib.Core.Version;
using CmlLib.Core.Installer.Forge.Installers;
using CmlLib.Core.Rules;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Install;
using SHLauncher.Contracts.Runtime;
namespace SHLauncher.Engine.CmlLib;
internal sealed class ModernLoaderInstallPipeline(IDownloadManager downloads, CmlLibMetadataService metadata,
    string allowedInstancesRoot, string metadataDirectory, string stagingRoot, IJavaRuntimeService java, IJavaSelectionService selection,
    IReadOnlyList<string> javaCandidates, SHLauncher.Contracts.Engine.IGameLogService processorLogs, string? manualJava = null)
{
    internal async Task<InstallResult> InstallAsync(LoaderInstallRequest request, Uri installerUri, IProgress<InstallProgress>? progress, CancellationToken token)
    {
        var root = PathSafety.EnsureContained(allowedInstancesRoot, request.DestinationRoot);
        var vanilla = await new CmlLibVanillaInstallService(downloads, metadata, allowedInstancesRoot).InstallAsync(new(request.Id, request.MinecraftVersion, root), progress, token).ConfigureAwait(false);
        var baseVersion = await LocalVersionSource.ReadAsync(root, request.MinecraftVersion, token).ConfigureAwait(false);
        try
        {
            var runtimes = await java.DetectAsync(javaCandidates, token).ConfigureAwait(false);
            var major = int.Parse(baseVersion.JavaVersion!.MajorVersion!);
            var runtime = selection.Select(major, System.Runtime.InteropServices.RuntimeInformation.OSArchitecture.ToString().ToLowerInvariant(), manualJava, runtimes).Runtime
                ?? throw new InvalidOperationException("Required Java runtime unavailable for Forge processors.");
            var stage = PathSafety.EnsureContained(stagingRoot, Path.Combine(stagingRoot, request.Id.ToString("N")));
            if (Directory.Exists(stage)) throw new IOException("Installation session must be new.");
            Directory.CreateDirectory(stage);
            var uri = installerUri;
            var hash = await new LoaderChecksumResolver(downloads, metadataDirectory).GetSha1Async(uri, request.Id, token).ConfigureAwait(false);
            var package = Path.Combine(stage, "installer.jar");
            var transfer = await downloads.DownloadAsync(new(request.Id, $"{request.Loader} installer", stage, [new(package, null, hash, null, [new(uri.Host, uri)])]), new Forward(progress), token).ConfigureAwait(false);
            token.ThrowIfCancellationRequested(); if (!transfer.Succeeded) throw new IOException("Loader installer download failed.");
            var extracted = Path.Combine(stage, "extracted"); await InstallerArchive.ExtractAsync(package, extracted, token).ConfigureAwait(false);
            // The mature processor mapping expects INSTALLER in the extraction directory.
            File.Copy(package, Path.Combine(extracted, "installer.jar"), false);
            using var profile = JsonDocument.Parse(await File.ReadAllTextAsync(Path.Combine(extracted, "install_profile.json"), token).ConfigureAwait(false));
            if (profile.RootElement.GetProperty("minecraft").GetString() != request.MinecraftVersion) throw new InvalidDataException("Installer Minecraft does not match selection.");
            var path = new MinecraftPath(root);
            await InstallerArchive.CopyMavenAsync(extracted, path.Library, token).ConfigureAwait(false);
            var bridge = new CoreFileDownloadAdapter(downloads, root, metadataDirectory, request.Id, progress);
            var libraries = new ForgeLibraryInstaller(bridge, new RulesEvaluator(), new RulesEvaluatorContext(LauncherOSRule.Current), "https://libraries.minecraft.net/");
            await libraries.Install(path, profile.RootElement.GetProperty("libraries"), null, null, token).ConfigureAwait(false);
            progress?.Report(new($"运行 {request.Loader} client processors"));
            await ForgeProcessorPipeline.RunAsync(runtime.ExecutablePath, extracted, path.GetVersionJarPath(request.MinecraftVersion), path.Library, root, profile.RootElement,
                value => processorLogs.Write(request.Id, SHLauncher.Contracts.Engine.GameLogLevel.Information, "installer", value), token).ConfigureAwait(false);
            using var input = File.OpenRead(Path.Combine(extracted, "version.json"));
            var version = StrictVersionParser.Read(input);
            try
            {
                if (version.InheritsFrom != request.MinecraftVersion) throw new InvalidDataException("Loader inheritance does not match selection.");
                version.ParentVersion = baseVersion;
                var plan = await CmlLibFileVerifier.PlanAsync(path, version, token).ConfigureAwait(false);
                await bridge.Install(plan, null, null, token).ConfigureAwait(false);
                foreach (var file in plan) await CmlLibFileVerifier.CheckAsync(path, file, token).ConfigureAwait(false);
                var destination = PathSafety.EnsureContained(root, path.GetVersionJsonPath(version.Id));
                Directory.CreateDirectory(Path.GetDirectoryName(destination)!);
                var part = destination + "." + request.Id.ToString("N") + ".part";
                File.Copy(Path.Combine(extracted, "version.json"), part, false);
                token.ThrowIfCancellationRequested(); File.Move(part, destination, true);
                progress?.Report(new($"{request.Loader} 安装完整性检查完成"));
                return new(request.Id, version.Id, root, plan.Count, vanilla.NetworkBytes + bridge.NetworkBytes + transfer.Files.Sum(f => f.NetworkBytes), bridge.LastReusedFiles);
            }
            finally { (version as IDisposable)?.Dispose(); }
        }
        finally { LocalVersionSource.Dispose(baseVersion); }
    }
    private sealed class Forward(IProgress<InstallProgress>? progress) : IProgress<DownloadTaskSnapshot>
    { public void Report(DownloadTaskSnapshot value) => progress?.Report(new("Loader 下载", value)); }
}
