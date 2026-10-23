using SHLauncher.Contracts.Downloads;
namespace SHLauncher.Contracts.Install;
public enum LoaderType { Vanilla, Fabric, Forge, NeoForge, Quilt }
public enum MinecraftVersionKind { Release, Snapshot, OldBeta, OldAlpha, Special }
public sealed record MinecraftVersionInfo(string Id, MinecraftVersionKind Kind, DateTimeOffset ReleasedAt, bool OfficialLatestRelease);
public interface IJsonMetadataSource { Task<Stream> OpenReadAsync(Uri uri, Guid sessionId, CancellationToken token); }
public interface IMinecraftVersionService { Task<IReadOnlyList<MinecraftVersionInfo>> GetVersionsAsync(CancellationToken token); }
public sealed record InstallRequest(Guid Id, string MinecraftVersion, string DestinationRoot);
public sealed record InstallProgress(string Stage, DownloadTaskSnapshot? Download = null);
public sealed record InstallResult(Guid Id, string InstanceId, string Root, int FileCount, long NetworkBytes, int ReusedFiles);
public interface IVanillaInstallService
{
    Task<InstallResult> InstallAsync(InstallRequest request, IProgress<InstallProgress>? progress, CancellationToken token);
}
