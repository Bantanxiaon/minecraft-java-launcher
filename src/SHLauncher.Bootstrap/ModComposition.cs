using SHLauncher.Contracts.Content;
namespace SHLauncher.Bootstrap;
public sealed partial class LauncherComposition
{
    private static string ModLockRoot(SHLauncher.Contracts.Engine.InstanceInfo instance)
    {
        var root=instance.Location?.Root??throw new IOException("实例位置不可用。");var parent=Path.GetDirectoryName(root);
        return parent is not null&&Guid.TryParseExact(instance.Id,"N",out _)&&Path.GetFileName(parent)==instance.Id?parent:root;
    }
    private bool OwnsModInstance(SHLauncher.Contracts.Engine.InstanceInfo instance)
    {
        if(IsMock||!Guid.TryParseExact(instance.Id,"N",out _)||instance.Location is not {} location)return false;
        var folder=Path.GetFullPath(Path.Combine(_workspace,"instances",instance.Id));
        var comparison=OperatingSystem.IsWindows()?StringComparison.OrdinalIgnoreCase:StringComparison.Ordinal;
        return Coordinator.Instances.Any(item=>item==instance)&&
            string.Equals(Path.GetFullPath(location.Root),Path.Combine(folder,"runtime"),comparison)&&
            string.Equals(Path.GetFullPath(location.GameDirectory),Path.Combine(folder,"game"),comparison)&&
            File.Exists(Path.Combine(folder,"instance.json"));
    }
    private IInstanceModService? _mods;
    public IInstanceModService Mods=>_mods??=new SHLauncher.Infrastructure.Content.InstanceModService(new SHLauncher.Platform.RecycleService(),new SHLauncher.Platform.PathOpener(),instance=>_instanceAccess.For(ModLockRoot(instance)),()=>Coordinator.Snapshot.State is SHLauncher.Core.Launching.LauncherTaskState.Idle or SHLauncher.Core.Launching.LauncherTaskState.Failed,OwnsModInstance);
}
