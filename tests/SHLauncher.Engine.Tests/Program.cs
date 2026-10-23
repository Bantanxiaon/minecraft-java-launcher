using System.Text.Json;
using SHLauncher.Engine.CmlLib;
using SHLauncher.Infrastructure.Runtime;
using SHLauncher.Application.Runtime;
using SHLauncher.Contracts.Runtime;
var token = CancellationToken.None;
if (args.Length > 0 && args[0] == "portable-package-smoke")
{
    await PortablePackageSmoke.Run(args[1]);
}
else if (args.Length > 0 && args[0] == "online-catalog-live")
{
    await OnlineCatalogLiveScenarios.Run(args[1]);
}
else if (args.Length > 0 && args[0] == "library-restore")
{
    await LibraryRestoreScenarios.Run(args[1]);
}
else if (args.Length > 0 && args[0] == "auth-agent")
{
    await AuthlibScenarios.Run(args[1]);
}
else if (args.Length > 0 && args[0] == "credential-environment-probe")
{
    if (new[] { "SH_CURSEFORGE_API_KEY", "SH_CURSEFORGE_CONFIG_FILE" }.Any(k => Environment.GetEnvironmentVariable(k) is not null))
        throw new Exception("Provider startup setting inherited by subprocess.");
    Console.WriteLine("Credential environment clear");
}
else if (args.Length > 0 && args[0] is "real-pack-auth" or "real-pack-install")
{
    await RealPackWorkflow.Run(args[0],args[1],args[2],args[3],args[4]);
}
else if (args.Length > 0 && args[0] == "credential-config-probe")
{
    await CredentialBoundaryScenarios.Probe(args[1],args[2]);
}
else if (args.Length > 0 && args[0] == "pack-access-gate")
{
    await ModpackAccessGate.Run(args[1],args[2],args[3]);
}
else if (args.Length > 0 && args[0] == "source-probe")
{
    Directory.CreateDirectory(args[1]);
    using var scheduler=new SHLauncher.Downloads.DownloadScheduler();
    using var journal=new SHLauncher.Downloads.JsonDownloadEventSink(Path.Combine(args[1],"logs"));
    using var downloads=new SHLauncher.Downloads.DownloadManager(scheduler,Path.Combine(args[1],"cache"),eventSink:journal,
        sourceSelector:new SHLauncher.Downloads.DownloadSourceSelector(()=>SHLauncher.Contracts.Downloads.DownloadSourceMode.MirrorPreferred,true));
    var id=Guid.NewGuid();
    var result=await downloads.DownloadAsync(new(id,"Real mirror probe",args[1],[
        new(Path.Combine(args[1],"artifact"),long.Parse(args[4]),args[3],null,[new("Mojang asset",new Uri(args[2]))])]),null,token);
    await File.WriteAllTextAsync(Path.Combine(args[1],"result.json"),JsonSerializer.Serialize(new{Result=result,Events=downloads.Events(id)}));
    if(!result.Succeeded)throw new Exception("Mirror and official probe failed.");
    Console.WriteLine(JsonSerializer.Serialize(result));
}
else if (args.Length > 0 && args[0] == "managed-storage")
{
    await ManagedStorageScenarios.Run(args[1],args[2]);
}
else if (args.Length > 0 && args[0] == "inspect-pack")
{
    var result = await new SHLauncher.Infrastructure.Install.CurseForgePackInspector().InspectAsync(args[1],token);
    await File.WriteAllTextAsync(args[2],JsonSerializer.Serialize(result));
    Console.WriteLine(JsonSerializer.Serialize(new {result.DisplayName,result.MinecraftVersion,result.Loader,result.LoaderVersion,Remote=result.RemoteFiles.Count,Local=result.LocalFiles.Count,result.ArchiveSha256}));
}
else if (args.Length > 0 && args[0] == "warm-reinstall")
{
    await WarmInstallScenarios.Run(args[1], args[2], args[3], args[4]);
}
else if (args.Length > 0 && args[0] == "metadata")
{
    using var scheduler = new SHLauncher.Downloads.DownloadScheduler();
    using var downloads = new SHLauncher.Downloads.DownloadManager(scheduler, Path.Combine(args[1], "cache"));
    var source = new SHLauncher.Downloads.JsonMetadataSource(downloads, Path.Combine(args[1], "metadata"));
    using var stream = await source.OpenReadAsync(new Uri(args[2]), Guid.NewGuid(), token);
    using var reader = new StreamReader(stream); Console.WriteLine(await reader.ReadToEndAsync(token));
}
else if (args.Length > 0 && args[0] == "scan")
{
    var results = await new CmlLibInstanceService().ScanAsync(args[1], token);
    Console.WriteLine(JsonSerializer.Serialize(results, new JsonSerializerOptions { WriteIndented = true }));
}
else if (args.Length > 0 && args[0] is "catalog" or "install-vanilla")
{
    var workspace = args[1]; Directory.CreateDirectory(workspace);
    using var scheduler = new SHLauncher.Downloads.DownloadScheduler();
    using var downloads = new SHLauncher.Downloads.DownloadManager(scheduler, Path.Combine(workspace, "cache"));
    var versions = new CmlLibMetadataService(new SHLauncher.Downloads.JsonMetadataSource(downloads, Path.Combine(workspace, "metadata")));
    var catalog = await versions.GetVersionsAsync(token);
    var choices = SHLauncher.Application.Versions.MinecraftVersionResolver.List(catalog);
    await File.WriteAllTextAsync(Path.Combine(workspace, "catalog.json"), JsonSerializer.Serialize(choices));
    var selected = args.Length > 2 ? choices.Single(v => v.Id == args[2]) : SHLauncher.Application.Versions.MinecraftVersionResolver.Recommended(catalog)!;
    Console.WriteLine("Official release selected: " + selected.Id);
    if (args[0] == "install-vanilla")
    {
        var id = Guid.NewGuid(); var root = Path.Combine(workspace, "instances", args.Length > 3 ? args[3] : "vanilla-" + selected.Id);
        var clock = System.Diagnostics.Stopwatch.StartNew();
        try
        {
            var result = await new CmlLibVanillaInstallService(downloads, versions, Path.Combine(workspace, "instances"))
                .InstallAsync(new(id, selected.Id, root), new InstallConsoleProgress(), token);
            await File.WriteAllTextAsync(Path.Combine(workspace, "vanilla-install.json"), JsonSerializer.Serialize(new { Result = result, ElapsedSeconds = clock.Elapsed.TotalSeconds }));
            Console.WriteLine(JsonSerializer.Serialize(result));
        }
        finally { await File.WriteAllTextAsync(Path.Combine(workspace, "download-events.json"), JsonSerializer.Serialize(downloads.Events(id))); }
    }
}
else if (args.Length > 0 && args[0] is "fabric-catalog" or "install-fabric")
{
    var workspace = args[1]; Directory.CreateDirectory(workspace);
    using var scheduler = new SHLauncher.Downloads.DownloadScheduler();
    using var downloads = new SHLauncher.Downloads.DownloadManager(scheduler, Path.Combine(workspace, "cache"));
    var versions = new CmlLibMetadataService(new SHLauncher.Downloads.JsonMetadataSource(downloads, Path.Combine(workspace, "metadata")));
    var adapter = new FabricInstallAdapter(downloads, versions, args[3], Path.Combine(workspace, "metadata"));
    var choices = await adapter.GetVersionsAsync(SHLauncher.Contracts.Install.LoaderType.Fabric, args[2], token);
    await File.WriteAllTextAsync(Path.Combine(workspace, "fabric-catalog.json"), JsonSerializer.Serialize(choices));
    Console.WriteLine(JsonSerializer.Serialize(choices.Take(3)));
    if (args[0] == "install-fabric")
    {
        var chosen = choices.Where(v => v.IsStable).OrderByDescending(v => System.Version.TryParse(v.Version, out var parsed) ? parsed : new System.Version()).First();
        Console.WriteLine("Selected: " + chosen.Version + "; " + chosen.Reason);
        var id = Guid.NewGuid(); var clock = System.Diagnostics.Stopwatch.StartNew();
        try {
            var result = await adapter.InstallAsync(new(id, args[2], Path.Combine(args[3], args.Length > 4 ? args[4] : "fabric-" + args[2]), chosen.Loader, chosen.Version), new InstallConsoleProgress(), token);
            await File.WriteAllTextAsync(Path.Combine(workspace, "fabric-install.json"), JsonSerializer.Serialize(new { Result = result, ElapsedSeconds = clock.Elapsed.TotalSeconds, Selected = chosen }));
            Console.WriteLine(JsonSerializer.Serialize(result));
        }
        finally { await File.WriteAllTextAsync(Path.Combine(workspace, "fabric-events.json"), JsonSerializer.Serialize(downloads.Events(id))); }
    }
}
else if (args.Length > 0 && args[0] == "forge-catalog")
{
    using var scheduler = new SHLauncher.Downloads.DownloadScheduler();
    using var downloads = new SHLauncher.Downloads.DownloadManager(scheduler, Path.Combine(args[1], "cache"));
    var json = new SHLauncher.Downloads.JsonMetadataSource(downloads, Path.Combine(args[1], "metadata"));
    var xml = new SHLauncher.Downloads.XmlMetadataSource(downloads, Path.Combine(args[1], "metadata"));
    var choices = await new ForgeVersionService(json, xml).GetVersionsAsync(SHLauncher.Contracts.Install.LoaderType.Forge, args[2], token);
    await File.WriteAllTextAsync(Path.Combine(args[1], "forge-catalog.json"), JsonSerializer.Serialize(choices));
    Console.WriteLine(JsonSerializer.Serialize(choices.Where(v => v.IsRecommended)));
}
else if (args.Length > 0 && args[0] == "forge-package")
{
    var root = args[1]; Directory.CreateDirectory(root);
    using var scheduler = new SHLauncher.Downloads.DownloadScheduler();
    using var downloads = new SHLauncher.Downloads.DownloadManager(scheduler, Path.Combine(root, "cache"));
    var id = Guid.NewGuid();
    var source = new Uri("https://maven.minecraftforge.net/net/minecraftforge/forge/" + args[2] + "/forge-" + args[2] + "-installer.jar");
    var checksum = Path.Combine(root, "installer.sha1");
    var hashResult = await downloads.DownloadAsync(new(id, "Official Forge installer checksum", root,
        [new(checksum, null, null, null, [new("Forge Maven", new Uri(source.AbsoluteUri + ".sha1"))], SHLauncher.Contracts.Downloads.DownloadContentKind.Sha1Metadata)]), null, token);
    if (!hashResult.Succeeded) throw new IOException(JsonSerializer.Serialize(hashResult));
    var hash = (await File.ReadAllTextAsync(checksum, token)).Trim();
    var result = await downloads.DownloadAsync(new(id, "Official Forge installer", root,
        [new(Path.Combine(root, "installer.jar"), null, hash, null, [new("Forge Maven", source)])]), null, token);
    await File.WriteAllTextAsync(Path.Combine(root, "download.json"), JsonSerializer.Serialize(result));
    if (!result.Succeeded) throw new IOException(JsonSerializer.Serialize(result));
    Console.WriteLine(JsonSerializer.Serialize(result));
}
else if (args.Length > 0 && args[0] == "install-forge")
{
    var work = args[1]; var instances = args[3]; var id = Guid.NewGuid();
    using var scheduler = new SHLauncher.Downloads.DownloadScheduler();
    using var downloads = new SHLauncher.Downloads.DownloadManager(scheduler, Path.Combine(work, "cache"));
    var json = new SHLauncher.Downloads.JsonMetadataSource(downloads, Path.Combine(work, "metadata"));
    var xml = new SHLauncher.Downloads.XmlMetadataSource(downloads, Path.Combine(work, "metadata"));
    var versions = new ForgeVersionService(json, xml);
    var choices = await versions.GetVersionsAsync(SHLauncher.Contracts.Install.LoaderType.Forge, args[2], token);
    var chosen = choices.Single(v => v.IsRecommended);
    Console.WriteLine(JsonSerializer.Serialize(chosen));
    var clock = System.Diagnostics.Stopwatch.StartNew();
    var processorLogs = new SHLauncher.Infrastructure.Logging.GameLogService();
    var adapter = new ForgeInstallAdapter(downloads, new CmlLibMetadataService(json), versions, instances, Path.Combine(work, "metadata"), Path.Combine(work, "staging"),
        new JavaRuntimeService(), new JavaSelectionService(), [args[4]], processorLogs);
    try
    {
        var result = await adapter.InstallAsync(new(id, args[2], Path.Combine(instances, args.Length > 5 ? args[5] : "forge-" + args[2]), chosen.Loader, chosen.Version), new InstallConsoleProgress(), token);
        await File.WriteAllTextAsync(Path.Combine(work, "forge-install.json"), JsonSerializer.Serialize(new { Result = result, Selected = chosen, ElapsedSeconds = clock.Elapsed.TotalSeconds }));
        Console.WriteLine(JsonSerializer.Serialize(result));
    }
    finally {
        await File.WriteAllTextAsync(Path.Combine(work, "forge-events.json"), JsonSerializer.Serialize(downloads.Events(id)));
        await File.WriteAllTextAsync(Path.Combine(work, "forge-processor.json"), JsonSerializer.Serialize(processorLogs.Snapshot(id)));
    }
}
else if (args.Length > 0 && args[0] is "neoforge-catalog" or "install-neoforge")
{
    var work = args[1]; var id = Guid.NewGuid();
    using var scheduler = new SHLauncher.Downloads.DownloadScheduler();
    using var downloads = new SHLauncher.Downloads.DownloadManager(scheduler, Path.Combine(work, "cache"));
    var json = new SHLauncher.Downloads.JsonMetadataSource(downloads, Path.Combine(work, "metadata"));
    var metadata = new CmlLibMetadataService(json);
    var versions = new NeoForgeVersionService(metadata);
    var choices = await versions.GetAllAsync(token);
    await File.WriteAllTextAsync(Path.Combine(work, "neoforge-catalog.json"), JsonSerializer.Serialize(choices));
    var minecraft = SHLauncher.Application.Versions.MinecraftVersionResolver.List(await metadata.GetVersionsAsync(token));
    var compatible = minecraft.First(v => choices.Any(n => n.MinecraftVersion == v.Id && n.IsStable));
    var chosen = choices.Where(v => v.MinecraftVersion == compatible.Id && v.IsStable).OrderByDescending(v => System.Version.Parse(v.Version)).First();
    Console.WriteLine(JsonSerializer.Serialize(chosen));
    if (args[0] == "install-neoforge")
    {
        var processorLogs = new SHLauncher.Infrastructure.Logging.GameLogService();
        var clock = System.Diagnostics.Stopwatch.StartNew();
        var adapter = new NeoForgeInstallAdapter(downloads, metadata, versions, args[2], Path.Combine(work, "metadata"), Path.Combine(work, "staging"),
            new JavaRuntimeService(), new JavaSelectionService(), [args[3]], processorLogs);
        try
        {
            var result = await adapter.InstallAsync(new(id, chosen.MinecraftVersion, Path.Combine(args[2], "neoforge-" + chosen.MinecraftVersion + "-cold1"), chosen.Loader, chosen.Version), new InstallConsoleProgress(), token);
            await File.WriteAllTextAsync(Path.Combine(work, "neoforge-install.json"), JsonSerializer.Serialize(new { Result = result, Selected = chosen, ElapsedSeconds = clock.Elapsed.TotalSeconds }));
            Console.WriteLine(JsonSerializer.Serialize(result));
        }
        finally {
            await File.WriteAllTextAsync(Path.Combine(work, "neoforge-events.json"), JsonSerializer.Serialize(downloads.Events(id)));
            await File.WriteAllTextAsync(Path.Combine(work, "neoforge-processor.json"), JsonSerializer.Serialize(processorLogs.Snapshot(id)));
        }
    }
}
else if (args.Length > 0 && args[0] is "quilt-catalog" or "install-quilt")
{
    var work = args[1]; var id = Guid.NewGuid();
    using var scheduler = new SHLauncher.Downloads.DownloadScheduler();
    using var downloads = new SHLauncher.Downloads.DownloadManager(scheduler, Path.Combine(work, "cache"));
    var metadata = new CmlLibMetadataService(new SHLauncher.Downloads.JsonMetadataSource(downloads, Path.Combine(work, "metadata")));
    var adapter = new QuiltInstallAdapter(downloads, metadata, args.Length > 2 ? args[2] : Path.Combine(work, "instances"), Path.Combine(work, "metadata"));
    var supported = await adapter.GetSupportedMinecraftAsync(token);
    var compatible = SHLauncher.Application.Versions.MinecraftVersionResolver.List(await metadata.GetVersionsAsync(token)).First(v => supported.Contains(v.Id));
    var choices = await adapter.GetVersionsAsync(SHLauncher.Contracts.Install.LoaderType.Quilt, compatible.Id, token);
    await File.WriteAllTextAsync(Path.Combine(work, "quilt-catalog.json"), JsonSerializer.Serialize(choices));
    var chosen = choices.Where(v => v.IsStable).OrderByDescending(v => System.Version.Parse(v.Version)).First();
    Console.WriteLine(JsonSerializer.Serialize(chosen));
    if (args[0] == "install-quilt")
    {
        var clock = System.Diagnostics.Stopwatch.StartNew();
        try {
            var result = await adapter.InstallAsync(new(id, chosen.MinecraftVersion, Path.Combine(args[2], "quilt-" + chosen.MinecraftVersion + "-cold1"), chosen.Loader, chosen.Version), new InstallConsoleProgress(), token);
            await File.WriteAllTextAsync(Path.Combine(work, "quilt-install.json"), JsonSerializer.Serialize(new { Result = result, Selected = chosen, ElapsedSeconds = clock.Elapsed.TotalSeconds }));
            Console.WriteLine(JsonSerializer.Serialize(result));
        }
        finally { await File.WriteAllTextAsync(Path.Combine(work, "quilt-events.json"), JsonSerializer.Serialize(downloads.Events(id))); }
    }
}
else if (args.Length > 0 && args[0] == "inspect")
{
    var files = await ExistingFileInspection.InspectAsync(args[1], args[2], token);
    await File.WriteAllTextAsync(args[3], JsonSerializer.Serialize(files, new JsonSerializerOptions { WriteIndented = true }));
    Console.WriteLine($"Files={files.Count}, invalid={files.Count(x => !x.Valid)}, bytes={files.Sum(x => x.Size)}");
    foreach (var invalid in files.Where(x => !x.Valid).Take(10)) Console.WriteLine(invalid.Problem);
}
else if (args.Length > 0 && args[0] == "launch")
{
    var logs = new SHLauncher.Infrastructure.Logging.GameLogService();
    var id = Guid.NewGuid(); var cancel = new CancellationTokenSource();
    var engine = new CmlLibLauncherEngine(new CmlLibInstanceService(), new JavaRuntimeService(), new JavaSelectionService(),
        new SHLauncher.Infrastructure.Accounts.OfflineAccountProvider(), logs, args[1], args[3], [args[4]], args[4], usePersistentGameData: true);
    try
    {
        await foreach (var update in engine.LaunchAsync(new(args[2], id), cancel.Token))
        {
            Console.WriteLine(JsonSerializer.Serialize(new { SessionId = id, update.State, update.Message }));
            await File.WriteAllTextAsync(args[5], JsonSerializer.Serialize(logs.Snapshot(id), new JsonSerializerOptions { WriteIndented = true }));
            if (update.State == SHLauncher.Core.Launching.LauncherTaskState.Running && args.Length > 6 && args[6] == "stop") cancel.CancelAfter(12000);
        }
    }
    catch (OperationCanceledException) { Console.WriteLine("Stopped; cleanup awaited."); }
    finally { await File.WriteAllTextAsync(args[5], JsonSerializer.Serialize(logs.Snapshot(id), new JsonSerializerOptions { WriteIndented = true })); }
}
else if (args.Length > 0 && args[0] == "java")
{
    var runtimes = await new JavaRuntimeService().DetectAsync(args.Skip(1), token);
    Console.WriteLine(JsonSerializer.Serialize(runtimes, new JsonSerializerOptions { WriteIndented = true }));
    var selection = new JavaSelectionService();
    foreach (var major in new[] { 17, 21, 25 })
        if (selection.Select(major, "x64", null, runtimes).Runtime is null) throw new Exception("Runtime not selected: " + major);
    if (selection.Select(17, "x64", "missing", runtimes).Runtime is not null) throw new Exception("Manual mismatch silently fell back.");
    if ((await new JavaRuntimeService().ProbeAsync(Path.Combine(Path.GetTempPath(), "missing-java.exe"), token)).IsUsable) throw new Exception("Missing Java accepted.");
}
else
{
    var fake = new JavaRuntime("test", "17.0.1", 17, "x64", "test", true);
    var selection = new JavaSelectionService();
    if (selection.Select(21, "x64", null, [fake]).Runtime is not null) throw new Exception("Wrong major accepted.");
    if (selection.Select(17, "arm64", null, [fake]).Runtime is not null) throw new Exception("Wrong architecture accepted.");
    if (selection.Select(0, "x64", null, [fake]).Runtime is not null) throw new Exception("Unknown requirement accepted.");
    PortableDataScenarios.Run();
    await FoundationScenarios.Run();
    await ArchiveScenarios.Run();
    await PackResolverScenarios.Run();
    await PackLayoutScenarios.Run();
    await PackDownloadScenarios.Run();
    await CredentialBoundaryScenarios.Run();
    await AccountScenarios.Run();
    Console.WriteLine("Engine foundation checks passed.");
}

sealed class InstallConsoleProgress : IProgress<SHLauncher.Contracts.Install.InstallProgress>
{
    private long _last;
    public void Report(SHLauncher.Contracts.Install.InstallProgress value)
    {
        var now = Environment.TickCount64;
        if (value.Download is not null && now - Interlocked.Read(ref _last) < 3000) return;
        Interlocked.Exchange(ref _last, now);
        Console.WriteLine(value.Download is { } task ? $"{task.State}: {task.CompletedFiles}/{task.TotalFiles}, {task.Progress.NetworkBytes / 1048576.0:F1} MiB, {task.Progress.BytesPerSecond / 1048576.0:F2} MiB/s" : value.Stage);
    }
}
