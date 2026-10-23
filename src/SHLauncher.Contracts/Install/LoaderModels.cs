namespace SHLauncher.Contracts.Install;
public sealed record LoaderVersionInfo(LoaderType Loader, string MinecraftVersion, string Version, bool IsStable, bool IsRecommended, string Reason);
public interface ILoaderVersionService
{
    Task<IReadOnlyList<LoaderVersionInfo>> GetVersionsAsync(LoaderType loader, string minecraftVersion, CancellationToken token);
}
public sealed record LoaderInstallRequest(Guid Id, string MinecraftVersion, string DestinationRoot, LoaderType Loader, string LoaderVersion);
public interface ILoaderInstallService
{
    Task<InstallResult> InstallAsync(LoaderInstallRequest request, IProgress<InstallProgress>? progress, CancellationToken token);
}
