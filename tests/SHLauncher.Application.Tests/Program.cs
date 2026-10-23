ArchitectureChecks.CheckProject("SHLauncher.Application", "SHLauncher.Contracts");
ArchitectureChecks.CheckProject("SHLauncher.UI", "SHLauncher.Application", "SHLauncher.Bootstrap");
await EngineScenarios.RunAll();
Console.WriteLine("PASS: Application architecture and mock lifecycle scenarios");

ArchitectureChecks.CheckProject("SHLauncher.Engine.CmlLib", "SHLauncher.Contracts");
ArchitectureChecks.CheckProject("SHLauncher.Bootstrap", "SHLauncher.Application", "SHLauncher.Infrastructure", "SHLauncher.Engine.CmlLib", "SHLauncher.Downloads", "SHLauncher.Platform");

ArchitectureChecks.CheckProject("SHLauncher.Downloads", "SHLauncher.Contracts");


var versions = new[] {
    new SHLauncher.Contracts.Install.MinecraftVersionInfo("1.20.2", SHLauncher.Contracts.Install.MinecraftVersionKind.Release, DateTimeOffset.UtcNow, false),
    new SHLauncher.Contracts.Install.MinecraftVersionInfo("1.20.10", SHLauncher.Contracts.Install.MinecraftVersionKind.Release, DateTimeOffset.UtcNow.AddDays(-1), true),
    new SHLauncher.Contracts.Install.MinecraftVersionInfo("26w01a", SHLauncher.Contracts.Install.MinecraftVersionKind.Snapshot, DateTimeOffset.UtcNow, false) };
var releases = SHLauncher.Application.Versions.MinecraftVersionResolver.List(versions);
if (releases.First().Id != "1.20.10" || releases.Count != 2 || SHLauncher.Application.Versions.MinecraftVersionResolver.Recommended(versions)?.Id != "1.20.10")
    throw new Exception("Minecraft resolution must use numeric release order and official latest release marker.");
Console.WriteLine("PASS: semantic numeric release ordering and official recommendation.");



await InstallationScenarios.Run();


await DiskTaskStoreScenarios.Run();

var loaderResolver = new SHLauncher.Application.Versions.VersionResolver();
var chosen = loaderResolver.Loaders([
 new(SHLauncher.Contracts.Install.LoaderType.Fabric,"1.20.1","0.9.0",true,false,"official stable"),
 new(SHLauncher.Contracts.Install.LoaderType.Fabric,"1.20.1","0.10.0",true,false,"official stable"),
 new(SHLauncher.Contracts.Install.LoaderType.Fabric,"1.20.2","99.0",true,true,"wrong MC"),
 new(SHLauncher.Contracts.Install.LoaderType.Fabric,"1.20.1","100.0",false,false,"preview")
], SHLauncher.Contracts.Install.LoaderType.Fabric,"1.20.1");
if (chosen.Count != 2 || chosen[0].Version != "0.10.0" || chosen.Any(v=>v.IsRecommended)) throw new Exception("Loader context and stable numeric preference.");
var compatibility = new SHLauncher.Application.Versions.CompatibilityService();
var content = new SHLauncher.Contracts.Content.ResolvedContentVersion(new("test","same-name"),"v1","1.0",
 SHLauncher.Contracts.Content.ReleaseChannel.Release,["1.20.1"],["Fabric"],[],DateTimeOffset.UtcNow);
if (compatibility.Check(content,new("1.20.1","Forge",null)).Code != SHLauncher.Contracts.Content.CompatibilityCode.WrongLoader ||
 compatibility.Check(content,new("1.20.2","Fabric",null)).Code != SHLauncher.Contracts.Content.CompatibilityCode.WrongMinecraftVersion ||
 compatibility.Check(content with { Loaders = [] },new("1.20.1","Fabric",null)).Code != SHLauncher.Contracts.Content.CompatibilityCode.Unknown)
 throw new Exception("Compatibility must not guess missing metadata.");
Console.WriteLine("PASS: contextual Loader resolution and explicit compatibility reasons/unknown.");

await ArchivePathScenarios.Run();

await AsyncCommandScenarios.Run();

await ProfileTextureScenarios.Run();

MemoryScenarios.Run();

await ModScenarios.Run();

await OnlineModScenarios.Run();

await OnlineModTransactionScenarios.Run();
