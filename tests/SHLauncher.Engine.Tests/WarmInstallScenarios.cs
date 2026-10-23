using System.Security.Cryptography;
using System.Text.Json;
using SHLauncher.Contracts.Install;
using SHLauncher.Engine.CmlLib;
using SHLauncher.Downloads;
using SHLauncher.Infrastructure.Runtime;
using SHLauncher.Application.Runtime;
using SHLauncher.Infrastructure.Logging;
internal static class WarmInstallScenarios
{
    internal sealed record Case(LoaderType Loader, string MinecraftVersion, string LoaderVersion, string Root, string InstanceId);
    internal static async Task Run(string work, string casesFile, string instances, string java)
    {
        var cases = JsonSerializer.Deserialize<Case[]>(await File.ReadAllTextAsync(casesFile))!;
        using var scheduler = new DownloadScheduler(); using var downloads = new DownloadManager(scheduler, Path.Combine(work, "cache"));
        var json = new JsonMetadataSource(downloads, Path.Combine(work, "metadata"));
        var xml = new XmlMetadataSource(downloads, Path.Combine(work, "metadata"));
        var metadata = new CmlLibMetadataService(json); var outcomes = new List<object>();
        foreach (var item in cases)
        {
            PathSafety.EnsureContained(instances, item.Root);
            var before = await Hashes(item.Root, false);
            var game = Path.Combine(work, "launch", "games", item.InstanceId); var gameBefore = await Hashes(game, true);
            var clock = System.Diagnostics.Stopwatch.StartNew(); var id = Guid.NewGuid();
            var request = new LoaderInstallRequest(id, item.MinecraftVersion, item.Root, item.Loader, item.LoaderVersion);
            InstallResult result;
            var logs = new GameLogService();
            if (item.Loader == LoaderType.Vanilla)
                result = await new CmlLibVanillaInstallService(downloads, metadata, instances).InstallAsync(new(id, item.MinecraftVersion, item.Root), null, default);
            else
            {
                ILoaderInstallService installer = item.Loader switch {
                    LoaderType.Fabric => new FabricInstallAdapter(downloads, metadata, instances, Path.Combine(work, "metadata")),
                    LoaderType.Quilt => new QuiltInstallAdapter(downloads, metadata, instances, Path.Combine(work, "metadata")),
                    LoaderType.Forge => new ForgeInstallAdapter(downloads, metadata, new ForgeVersionService(json, xml), instances, Path.Combine(work, "metadata"), Path.Combine(work, "staging"), new JavaRuntimeService(), new JavaSelectionService(), [java], logs),
                    LoaderType.NeoForge => new NeoForgeInstallAdapter(downloads, metadata, new NeoForgeVersionService(metadata), instances, Path.Combine(work, "metadata"), Path.Combine(work, "staging"), new JavaRuntimeService(), new JavaSelectionService(), [java], logs),
                    _ => throw new NotSupportedException() };
                result = await installer.InstallAsync(request, null, default);
            }
            var after = await Hashes(item.Root, false); var gameAfter = await Hashes(game, true);
            var same = before.Count == after.Count && before.All(pair => after.GetValueOrDefault(pair.Key) == pair.Value);
            var gameSame = gameBefore.Count == gameAfter.Count && gameBefore.All(pair => gameAfter.GetValueOrDefault(pair.Key) == pair.Value);
            var outcome = new { item.Loader, item.MinecraftVersion, item.LoaderVersion, Result = result, ArtifactHashesUnchanged = same,
                GameDataUnchanged = gameSame, FilesBefore = before.Count, FilesAfter = after.Count, ElapsedSeconds = clock.Elapsed.TotalSeconds };
            outcomes.Add(outcome); Console.WriteLine(JsonSerializer.Serialize(outcome));
            await File.WriteAllTextAsync(Path.Combine(work, "warm-results.json"), JsonSerializer.Serialize(outcomes));
            if (!same || !gameSame || result.NetworkBytes != 0) throw new Exception("Warm reinstall changed installed artifacts/game data or re-downloaded artifacts.");
        }
    }
    private static async Task<Dictionary<string,string>> Hashes(string root, bool all)
    {
        var result = new Dictionary<string,string>(); if (!Directory.Exists(root)) return result;
        foreach (var path in Directory.EnumerateFiles(root, "*", SearchOption.AllDirectories).Order())
        {
            if (!all && (path.EndsWith(".json", StringComparison.OrdinalIgnoreCase) || path.EndsWith(".part", StringComparison.OrdinalIgnoreCase))) continue;
            using var input = File.OpenRead(path); result.Add(Path.GetRelativePath(root, path), Convert.ToHexString(await SHA256.HashDataAsync(input)));
        }
        return result;
    }
}
