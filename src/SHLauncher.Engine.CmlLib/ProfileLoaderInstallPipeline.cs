using System.Text.Json;
using System.Text.Json.Nodes;
using CmlLib.Core;
using CmlLib.Core.Version;
using CmlLib.Core.FileExtractors;
using CmlLib.Core.Rules;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Install;
namespace SHLauncher.Engine.CmlLib;
internal sealed class ProfileLoaderInstallPipeline(IDownloadManager downloads, CmlLibMetadataService metadata, string allowedInstancesRoot, string metadataDirectory)

{
    internal async Task<InstallResult> InstallAsync(LoaderInstallRequest request, Func<Task<Stream>> getProfile, string expectedVersionId, IProgress<InstallProgress>? progress, CancellationToken token)
    {
        var root = PathSafety.EnsureContained(allowedInstancesRoot, request.DestinationRoot);
        var vanilla = await new CmlLibVanillaInstallService(downloads, metadata, allowedInstancesRoot)
            .InstallAsync(new(request.Id, request.MinecraftVersion, root), progress, token).ConfigureAwait(false);
        using var profile = await getProfile().ConfigureAwait(false);
        var document = await JsonNode.ParseAsync(profile, cancellationToken: token).ConfigureAwait(false) ?? throw new InvalidDataException("Empty Loader profile.");
        var versionId = document["id"]?.GetValue<string>() ?? throw new InvalidDataException("Profile has no id.");
        if (versionId != expectedVersionId || document["inheritsFrom"]?.GetValue<string>() != request.MinecraftVersion)
            throw new InvalidDataException("Loader profile identity does not match selection.");
        var path = new MinecraftPath(root);
        using var input = new MemoryStream(System.Text.Encoding.UTF8.GetBytes(document.ToJsonString()));
        var version = StrictVersionParser.Read(input);
        try
        {
            var plan = (await new LibraryFileExtractor(JsonVersionParserOptions.ClientSide, new RulesEvaluator()).Extract(path, version,
                new RulesEvaluatorContext(LauncherOSRule.Current), token).ConfigureAwait(false)).ToArray();
            var checksums = new LoaderChecksumResolver(downloads, metadataDirectory);
            var hashes = new Dictionary<string, string>();
            foreach (var file in plan)
            {
                token.ThrowIfCancellationRequested();
                PathSafety.EnsureContained(root, file.Path!);
                if (!Uri.TryCreate(file.Url, UriKind.Absolute, out var uri) || uri.Scheme != "https") throw new InvalidDataException("Loader library has no HTTPS source.");
                var hash = string.IsNullOrEmpty(file.Hash) ? await checksums.GetSha1Async(uri, request.Id, token).ConfigureAwait(false) : file.Hash;
                hashes.Add(file.Path!, hash);
                var node = document["libraries"]!.AsArray().Single(n => n?["name"]?.GetValue<string>() == file.Name)!;
                node["downloads"] = new JsonObject { ["artifact"] = new JsonObject {
                    ["url"] = file.Url, ["sha1"] = hash,
                    ["path"] = Path.GetRelativePath(path.Library, file.Path!).Replace(Path.DirectorySeparatorChar, '/') } };
            }
            progress?.Report(new($"下载并验证 {request.Loader} libraries"));
            var result = await downloads.DownloadAsync(new(request.Id, request.Loader + " " + request.LoaderVersion, root,
                plan.Select(f => new DownloadFile(f.Path!, f.Size > 0 ? f.Size : null, hashes[f.Path!], null, [new(new Uri(f.Url!).Host, new Uri(f.Url!))])).ToArray()),
                new DownloadProgressForwarder(progress), token).ConfigureAwait(false);
            token.ThrowIfCancellationRequested();
            if (!result.Succeeded) throw new IOException(result.Failures.FirstOrDefault()?.Message ?? "Loader libraries failed.");
            var destination = PathSafety.EnsureContained(root, path.GetVersionJsonPath(versionId));
            Directory.CreateDirectory(Path.GetDirectoryName(destination)!);
            var part = destination + "." + Guid.NewGuid().ToString("N") + ".part";
            await File.WriteAllTextAsync(part, document.ToJsonString(), token).ConfigureAwait(false);
            using (var verified = File.OpenRead(part)) LocalVersionSource.Dispose(StrictVersionParser.Read(verified));
            global::CmlLib.Core.Files.GameFile[] completePlan;
            DownloadResult complete;
            using (var parsedInput = File.OpenRead(part))
            {
                var staged = StrictVersionParser.Read(parsedInput);
                try
                {
                    staged.ParentVersion = await LocalVersionSource.ReadAsync(root, request.MinecraftVersion, token).ConfigureAwait(false);
                    completePlan = (await CmlLibFileVerifier.PlanAsync(path, staged, token).ConfigureAwait(false)).ToArray();
                    complete = await downloads.DownloadAsync(new(request.Id, $"{request.Loader} complete file graph", root,
                        completePlan.Select(f => new DownloadFile(f.Path!, f.Size > 0 ? f.Size : null, f.Hash, null, [new(new Uri(f.Url!).Host, new Uri(f.Url!))])).ToArray()),
                        new DownloadProgressForwarder(progress), token).ConfigureAwait(false);
                    token.ThrowIfCancellationRequested();
                    if (!complete.Succeeded) throw new IOException(complete.Failures.FirstOrDefault()?.Message ?? "Inherited file graph failed.");
                    foreach (var file in completePlan) await CmlLibFileVerifier.CheckAsync(path, file, token).ConfigureAwait(false);
                }
                finally { LocalVersionSource.Dispose(staged); }
            }
            token.ThrowIfCancellationRequested(); File.Move(part, destination, true);
            progress?.Report(new($"{request.Loader} 安装和继承文件检查完成"));
            return new(request.Id, versionId, root, completePlan.Length, vanilla.NetworkBytes + result.Files.Sum(f => f.NetworkBytes) + complete.Files.Sum(f => f.NetworkBytes), complete.Files.Count(f => f.Reused));
        }
        finally { LocalVersionSource.Dispose(version); }
    }
    private sealed class DownloadProgressForwarder(IProgress<InstallProgress>? progress) : IProgress<DownloadTaskSnapshot>
    { public void Report(DownloadTaskSnapshot value) => progress?.Report(new("Loader 下载", value)); }
}
