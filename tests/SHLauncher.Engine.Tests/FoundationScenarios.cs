using System.Text.Json;
using SHLauncher.Application.Content;
using SHLauncher.Application.Runtime;
using SHLauncher.Contracts.Accounts;
using SHLauncher.Contracts.Content;
using SHLauncher.Contracts.Engine;
using SHLauncher.Contracts.Runtime;
using SHLauncher.Core.Launching;
using SHLauncher.Engine.CmlLib;
using SHLauncher.Infrastructure.Content;
using SHLauncher.Infrastructure.Logging;
using SHLauncher.Infrastructure.Runtime;

internal static class FoundationScenarios
{
    internal static async Task Run()
    {
        var root = Path.Combine(ArchitectureRoot(), ".devdata", "fixtures", Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        var scanner = new CmlLibInstanceService();
        var token = CancellationToken.None;
        await Throws(LauncherErrorCode.InstanceNotFound, async () => { await scanner.ScanAsync(Path.Combine(root, "missing"), token); });
        await Throws(LauncherErrorCode.InstanceNotFound, async () => { await scanner.ScanAsync("relative", token); });
        Check((await scanner.ScanAsync(root, token)).Count == 0, "empty root");
        var versions = Path.Combine(root, "versions");
        string Metadata(string id, string json)
        {
            var folder = Path.Combine(versions, id); Directory.CreateDirectory(folder);
            var path = Path.Combine(folder, id + ".json"); File.WriteAllText(path, json); return path;
        }
        var corrupt = Metadata("corrupt", "{");
        Metadata("null-id", "{\"id\":null}");
        Directory.CreateDirectory(Path.Combine(versions, "missing-metadata"));
        var valid = Metadata("renamed", """
        {"id":"1.20.1","mainClass":"net.minecraft.client.main.Main","javaVersion":{"component":"test","majorVersion":17},"libraries":[]}
        """);
        File.WriteAllText(Path.Combine(versions, "renamed", "renamed.jar"), "not a real game");
        Metadata("bad-library", "{\"id\":\"bad-library\",\"mainClass\":\"net.minecraft.client.main.Main\",\"javaVersion\":{\"majorVersion\":17},\"libraries\":[{\"name\":null,\"downloads\":17}]}");
        Metadata("bad-argument", "{\"id\":\"bad-argument\",\"arguments\":{\"game\":[{\"value\":[]} ]}}");
        Metadata("bad-rule", "{\"id\":\"bad-rule\",\"libraries\":[{\"name\":\"org.example:demo:1.0\",\"rules\":[null]}]}");
        var before = Directory.GetFiles(root, "*", SearchOption.AllDirectories).ToDictionary(p => p, File.ReadAllBytes);
        var results = await scanner.ScanAsync(root, token);
        Check(results.Count == 7, "bad metadata does not drop sibling entries");
        Check(results.Single(x => x.Id == "bad-argument").Problem?.Code == LauncherErrorCode.InstanceInvalid, "dropped argument must be rejected");
        Check(results.Single(x => x.Id == "bad-rule").Problem?.Code == LauncherErrorCode.InstanceInvalid, "dropped rule must be rejected");
        Check(results.Single(x => x.Id == "bad-library").Problem?.Code == LauncherErrorCode.InstanceInvalid, "malformed lazy library data must not silently disappear");
        Check(results.Single(x => x.Id == "corrupt").Problem?.Code == LauncherErrorCode.InstanceInvalid, "invalid JSON");
        Check(results.Single(x => x.Id == "null-id").Problem?.Code == LauncherErrorCode.InstanceInvalid, "core parse error mapped");
        Check(results.Single(x => x.Id == "missing-metadata").Problem?.Code == LauncherErrorCode.MissingMetadata, "missing metadata");
        Check(results.Single(x => x.Id == "renamed").GameVersion == "1.20.1", "directory id differs from metadata id");
        Check(before.Count == Directory.GetFiles(root, "*", SearchOption.AllDirectories).Length && before.All(p => p.Value.SequenceEqual(File.ReadAllBytes(p.Key))), "scanner byte-for-byte read-only");
        var logs = new GameLogService(); var id = Guid.NewGuid();
        foreach (var message in new[] { "--accessToken secret-1", "Authorization: Bearer secret-2", "Cookie: secret-3", "refresh_token=secret-4", "XSTS=secret-5", "x-api-key: secret-7", "API_KEY=secret-8", "{\"accessToken\":\"secret-6\"}" })
            logs.Write(id, GameLogLevel.Information, "test", message);
        Check(logs.Snapshot().All(x => !x.Message.Contains("secret-", StringComparison.Ordinal)), "sensitive data rejected before buffering");
        for (var i = 0; i < 1200; i++) logs.Write(id, GameLogLevel.Information, "test", new string('x', 3000));
        Check(logs.Snapshot().Count == GameLogService.Capacity && logs.Snapshot().All(x => x.Message.Length <= 2049 && x.SessionId == id), "bounded log and session id");
        Check(logs.Snapshot(Guid.NewGuid()).Count == 0, "session log isolation");
        var selection = new JavaSelectionService();
        Check(selection.Select(17, "x64", root.Replace(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar),
            [new(root, "17.0.1", 17, "x64", "test", true)]).Runtime is not null, "manual absolute separators normalized");
        JavaRuntime Runtime(string version) => new(version, version, 17, "x64", "test", true);
        Check(selection.Select(17, "x64", null, [Runtime("17.0.9"), Runtime("17.0.10")]).Runtime?.Version == "17.0.10", "numeric Java patch sorting");
        Check(selection.Select(17, "x64", "unavailable", [Runtime("17.0.10")]).Issue == JavaSelectionIssue.Missing, "missing manual Java classified despite other installed runtimes");
        Check(!(await new JavaRuntimeService().ProbeAsync(Path.Combine(root, "java.exe"), token)).IsUsable, "missing Java");
        var engine = new CmlLibLauncherEngine(scanner, new NoJava(), selection, new TestAccount(), logs, root, Path.Combine(root, "work"), []);
        await Throws(LauncherErrorCode.RuntimeMissing, async () => { await foreach (var _ in engine.LaunchAsync(new("renamed", id), token)) { } });
        Check(!Directory.Exists(Path.Combine(root, "work")), "missing runtime does not write game files");
        using (var cancel = new CancellationTokenSource())
        {
            var cancelled = false;
            try { await foreach (var update in engine.LaunchAsync(new("renamed"), cancel.Token)) { Check(update.IsIndeterminate && update.Progress == 0, "unknown preparation has no fake percent"); cancel.Cancel(); } }
            catch (OperationCanceledException) { cancelled = true; }
            Check(cancelled, "prepare cancellation");
        }
        // Reuse after cancellation must release the ownership slot.
        await Throws(LauncherErrorCode.InstanceNotFound, async () => { await foreach (var _ in engine.LaunchAsync(new("not-present"), token)) { } });
        var cv = new ContentVersion("1", ["1.20.1"], ["Fabric"], [new("test", "dep", DependencyKind.Required)]);
        Check(ContentCompatibilityService.Evaluate(cv, "1.21", "Fabric", new HashSet<string>()).Status == CompatibilityStatus.IncompatibleMinecraftVersion, "content Minecraft mismatch");
        Check(ContentCompatibilityService.Evaluate(cv, "1.20.1", "Forge", new HashSet<string>()).Status == CompatibilityStatus.WrongLoader, "content Loader mismatch");
        Check(ContentCompatibilityService.Evaluate(cv, "1.20.1", "Fabric", new HashSet<string>()).Status == CompatibilityStatus.MissingDependency, "content required dependency");
        Check(ContentCompatibilityService.Evaluate(cv, "1.20.1", "Fabric", new HashSet<string> { "test:dep" }).Status == CompatibilityStatus.UnknownCompatibility, "content declarations compatible");
        Check(ContentCompatibilityService.Evaluate(cv with { MinecraftVersions = [] }, "1.20.1", "Fabric", new HashSet<string>()).Status == CompatibilityStatus.UnknownCompatibility, "content unknown");
        var paths = new LocalContentService().GetDirectories(root);
        Check(paths.Mods == Path.Combine(root, "mods") && !Directory.Exists(paths.Mods), "content path model creates nothing");
        Check((await new MockContentProvider().SearchAsync(new("test", ContentType.Mod, null, null), token)).Count == 0, "content mock does not network");
        Console.WriteLine("PASS: read-only scanner, malformed metadata, runtime selection, bounded private logs, cancel, ownership release, content compatibility.");
    }
    private static void Check(bool condition, string message) { if (!condition) throw new Exception(message); }
    private static async Task Throws(LauncherErrorCode code, Func<Task> action)
    {
        try { await action(); }
        catch (LauncherEngineException e) when (e.Error.Code == code) { return; }
        throw new Exception("Expected mapped error: " + code);
    }
    private static string ArchitectureRoot()
    {
        var folder = new DirectoryInfo(AppContext.BaseDirectory);
        while (folder is not null && !File.Exists(Path.Combine(folder.FullName, "SHLauncherNext.sln"))) folder = folder.Parent;
        return folder?.FullName ?? throw new Exception("Root missing");
    }
    private sealed class NoJava : IJavaRuntimeService
    {
        public Task<IReadOnlyList<JavaRuntime>> DetectAsync(IEnumerable<string> candidates, CancellationToken token) => Task.FromResult<IReadOnlyList<JavaRuntime>>([]);
        public Task<JavaRuntime> ProbeAsync(string executable, CancellationToken token) => throw new NotSupportedException();
    }
    private sealed class TestAccount : IAccountProvider
    { public Task<LaunchIdentity> GetAsync(CancellationToken token) => Task.FromResult(new LaunchIdentity("Test", Guid.NewGuid())); }
}
