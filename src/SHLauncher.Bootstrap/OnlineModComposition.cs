using SHLauncher.Contracts.Content;
namespace SHLauncher.Bootstrap;
public sealed partial class LauncherComposition
{
    private IOnlineModService? _onlineMods;
    public IOnlineModService OnlineMods=>_onlineMods??=new SHLauncher.Infrastructure.Content.OnlineModService(_downloads,Mods,instance=>_instanceAccess.For(ModLockRoot(instance)),()=>Coordinator.Snapshot.State is SHLauncher.Core.Launching.LauncherTaskState.Idle or SHLauncher.Core.Launching.LauncherTaskState.Failed,new SHLauncher.Application.Content.ModDependencyPlanner(OnlineModProviders).PlanAsync);
    private IReadOnlyList<IOnlineModProvider>? _onlineModProviders;
    public IReadOnlyList<IOnlineModProvider> OnlineModProviders=>_onlineModProviders??=CreateOnlineProviders();
    private IReadOnlyList<IOnlineModProvider> CreateOnlineProviders()
    {
        var metadata=new SHLauncher.Downloads.JsonMetadataSource(_downloads,Path.Combine(_workspace,"shared","metadata"));
        return [new SHLauncher.Infrastructure.Content.OnlineModProvider("Modrinth",metadata,()=>true),new SHLauncher.Infrastructure.Content.OnlineModProvider("CurseForge",metadata,HasCurseForgeAccess)];
    }
}
