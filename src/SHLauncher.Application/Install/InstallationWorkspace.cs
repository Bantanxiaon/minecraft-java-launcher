using SHLauncher.Application.Versions;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Install;
namespace SHLauncher.Application.Install;
public interface IInstallationWorkspace : IAsyncDisposable
{
    IDownloadSourcePreferences SourcePreferences { get; }
    InstallationCoordinator Installations { get; }
    VersionSelectionService Versions { get; }
    IModpackInspector ModpackInspector { get; }
    IReadOnlyList<DownloadEvent> DownloadEvents(Guid id);
    Task<string> RenameInstanceAsync(Guid id, string displayName, CancellationToken token);
    Task<string> OpenInstalledAsync(InstallResult result, CancellationToken token);
}
