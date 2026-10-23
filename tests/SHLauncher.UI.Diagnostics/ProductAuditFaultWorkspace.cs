using SHLauncher.Application.Install;
using SHLauncher.Application.Versions;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Install;
namespace SHLauncher.UI.Diagnostics;
// Diagnostic-only provider fault injection. The production resolver and transport are not changed.
internal sealed class ProductAuditFaultWorkspace(IInstallationWorkspace inner,IReadOnlyList<MinecraftVersionInfo> catalog):IInstallationWorkspace
{
    public IDownloadSourcePreferences SourcePreferences=>inner.SourcePreferences;
    public InstallationCoordinator Installations=>inner.Installations;
    public VersionSelectionService Versions {get;}=new(new Catalog(catalog),new FailedLoader(),new VersionResolver());
    public IModpackInspector ModpackInspector=>inner.ModpackInspector;
    public IReadOnlyList<DownloadEvent> DownloadEvents(Guid id)=>inner.DownloadEvents(id);
    public Task<string> RenameInstanceAsync(Guid id,string name,CancellationToken token)=>inner.RenameInstanceAsync(id,name,token);
    public Task<string> OpenInstalledAsync(InstallResult result,CancellationToken token)=>inner.OpenInstalledAsync(result,token);
    public ValueTask DisposeAsync()=>ValueTask.CompletedTask;
    private sealed class Catalog(IReadOnlyList<MinecraftVersionInfo> items):IMinecraftVersionService
    {public Task<IReadOnlyList<MinecraftVersionInfo>> GetVersionsAsync(CancellationToken token)=>Task.FromResult(items);}
    private sealed class FailedLoader:ILoaderVersionService
    {
        public Task<IReadOnlyList<LoaderVersionInfo>> GetVersionsAsync(LoaderType loader,string minecraft,CancellationToken token)=>
            Task.FromException<IReadOnlyList<LoaderVersionInfo>>(new IOException("Controlled provider metadata failure (503); diagnostic fixture, no network request."));
    }
}
