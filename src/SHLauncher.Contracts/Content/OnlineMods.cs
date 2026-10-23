using SHLauncher.Contracts.Downloads;
namespace SHLauncher.Contracts.Content;
public sealed record OnlineModFile(ResolvedContentVersion Version,string FileName,long Size,string Sha1,Uri DownloadUri);
public interface IOnlineModProvider:IContentProvider
{
    Task<IReadOnlyList<OnlineModFile>> VersionsAsync(string projectId,InstanceCompatibilityContext context,CancellationToken token);
    Task<OnlineModFile> ExactVersionAsync(string projectId,string versionId,CancellationToken token);
}

public sealed record ModInstallTarget(OnlineModFile Artifact,string Destination,string? PreviousFile,string? PreviousSha1,bool AlreadyInstalled);
public sealed record ModInstallPreview(Guid Id,string InstanceId,ContentInstallPlan Dependencies,IReadOnlyList<ModInstallTarget> Targets,IReadOnlyList<string> Problems,string Snapshot)
{public bool CanInstall=>Dependencies.CanInstall&&Problems.Count==0;}
public interface IOnlineModService
{
    Task RecoverAsync(SHLauncher.Contracts.Engine.InstanceInfo instance,CancellationToken token);
    Task<ModInstallPreview> PreviewAsync(SHLauncher.Contracts.Engine.InstanceInfo instance,OnlineModFile selected,CancellationToken token);
    Task InstallAsync(SHLauncher.Contracts.Engine.InstanceInfo instance,ModInstallPreview preview,IProgress<DownloadTaskSnapshot>? progress,CancellationToken token);
}
