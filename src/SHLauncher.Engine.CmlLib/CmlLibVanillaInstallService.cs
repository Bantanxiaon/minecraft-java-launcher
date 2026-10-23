using CmlLib.Core;
using CmlLib.Core.Version;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Install;
namespace SHLauncher.Engine.CmlLib;
public sealed class CmlLibVanillaInstallService(IDownloadManager downloads, CmlLibMetadataService versions, string allowedInstancesRoot) : IVanillaInstallService
{
    public async Task<InstallResult> InstallAsync(InstallRequest request, IProgress<InstallProgress>? progress, CancellationToken token)
    {
        var root = PathSafety.EnsureContained(allowedInstancesRoot, request.DestinationRoot);
        var manifest = await versions.ManifestAsync(request.Id, token).ConfigureAwait(false);
        var selected = manifest.Versions.SingleOrDefault(v => v.Id == request.MinecraftVersion && v.Type == "release") ?? throw new ArgumentException("Select an available official release.");
        if (Path.GetFileName(selected.Id) != selected.Id || selected.Id!.IndexOfAny(Path.GetInvalidFileNameChars()) >= 0) throw new InvalidDataException("Invalid version identifier.");
        var path = new MinecraftPath(root);
        var metadata = path.GetVersionJsonPath(selected.Id!);
        progress?.Report(new("获取并验证版本元数据"));
        await Download([new(metadata, selected.Size > 0 ? selected.Size : null, selected.GetSha1(), null,
            [new("Mojang", new Uri(selected.Url!))], DownloadContentKind.JsonMetadata)]).ConfigureAwait(false);
        IVersion version;
        using (var stream = File.OpenRead(metadata)) version = StrictVersionParser.Read(stream);
        try
        {
            if (version.InheritsFrom is not null || version.MainClass != "net.minecraft.client.main.Main") throw new InvalidDataException("Expected standalone Vanilla metadata.");
            var index = version.AssetIndex ?? throw new InvalidDataException("Asset index missing.");
            progress?.Report(new("下载并校验资产索引"));
            await Download([new(path.GetIndexFilePath(index.Id!), index.Size, index.GetSha1(), null,
                [new("Mojang", new Uri(index.Url!))], DownloadContentKind.JsonMetadata)]).ConfigureAwait(false);
            progress?.Report(new("解析核心文件图"));
            var plan = await Task.Run(() => CmlLibFileVerifier.PlanAsync(path, version, token), token).ConfigureAwait(false);
            var files = plan.Select(f =>
            {
                if (!Uri.TryCreate(f.Url, UriKind.Absolute, out var source)) throw new InvalidDataException("Core file plan has no downloadable source: " + f.Name);
                return new DownloadFile(f.Path!, f.Size > 0 ? f.Size : null, f.Hash, null, [new(source.Host, source)]);
            }).ToArray();
            progress?.Report(new("下载游戏、依赖库与资产"));
            var result = await Download(files).ConfigureAwait(false);
            progress?.Report(new("验证完整安装"));
            foreach (var file in plan) await CmlLibFileVerifier.CheckAsync(path, file, token).ConfigureAwait(false);
            progress?.Report(new("原版安装完成"));
            return new(request.Id, selected.Id!, root, files.Length, result.Files.Sum(f => f.NetworkBytes), result.Files.Count(f => f.Reused));
        }
        finally { LocalVersionSource.Dispose(version); }
        async Task<DownloadResult> Download(IReadOnlyList<DownloadFile> files)
        {
            var result = await downloads.DownloadAsync(new(request.Id, "Minecraft " + request.MinecraftVersion, root, files),
                new ForwardProgress(value => progress?.Report(new("下载任务", value))), token).ConfigureAwait(false);
            token.ThrowIfCancellationRequested();
            if (!result.Succeeded) throw new IOException(result.Failures.FirstOrDefault()?.Message ?? "Download failed.");
            return result;
        }
    }
    private sealed class ForwardProgress(Action<DownloadTaskSnapshot> action) : IProgress<DownloadTaskSnapshot>
    { public void Report(DownloadTaskSnapshot value) => action(value); }
}
