using SHLauncher.Contracts.Engine;
namespace SHLauncher.Contracts.Content;
public sealed record ModDependency(string Id,string Constraint,bool Required);
public sealed record LocalMod(string FileName,string Path,bool Enabled,string Id,string Name,string Version,string Loader,IReadOnlyList<string> Authors,string Description,IReadOnlyList<ModDependency> Dependencies,byte[]? Icon,string? Problem,long Size,DateTime LastWriteUtc);
public sealed record ModOperationResult(string FileName,bool Succeeded,string Message);
public interface IRecycleService{Task RecycleAsync(string path,CancellationToken token);}
public interface IPathOpener{void OpenFolder(string directory);}
public interface IInstanceModService
{
    int ClearMetadataCache();
    bool CanManage(InstanceInfo instance);
    Task<IReadOnlyList<LocalMod>> ScanAsync(InstanceInfo instance,bool force,CancellationToken token);
    Task<IReadOnlyList<ModOperationResult>> SetEnabledAsync(InstanceInfo instance,IReadOnlyList<string> files,bool enabled,CancellationToken token);
    Task<IReadOnlyList<ModOperationResult>> InstallLocalAsync(InstanceInfo instance,IReadOnlyList<string> paths,CancellationToken token);
    Task<IReadOnlyList<ModOperationResult>> RecycleAsync(InstanceInfo instance,IReadOnlyList<string> files,CancellationToken token);
    IDisposable Watch(InstanceInfo instance,Action changed);
    void OpenFolder(InstanceInfo instance);
}
