using SHLauncher.Application.Runtime;
using SHLauncher.Application.Install;
using SHLauncher.Application.Versions;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Install;
using SHLauncher.Contracts.Instances;
using SHLauncher.Contracts.Engine;
using SHLauncher.Downloads;
using SHLauncher.Engine.CmlLib;
using SHLauncher.Infrastructure.Runtime;
using SHLauncher.Infrastructure.Accounts;
using SHLauncher.Infrastructure.Install;
namespace SHLauncher.Bootstrap;
public sealed partial class LauncherComposition
{
    private readonly InstanceAccess _instanceAccess = new();
    private FileStream _workspaceLease = null!;
    private DownloadScheduler _scheduler = null!;
    private DownloadManager _downloads = null!;
    private JsonDownloadEventSink _journal = null!;
    private CmlLibInstallationBackend _backend = null!;
    public IDownloadSourcePreferences SourcePreferences { get; private set; } = null!;
    public InstallationCoordinator Installations { get; private set; } = null!;
    public IModpackInspector ModpackInspector { get; } = new CurseForgePackInspector();
    public VersionSelectionService Versions { get; private set; } = null!;
    private void InitializeInstallations()
    {
        InitializeCurseForgeAccess();
        ArchivePathPolicy.Resolve(_workspace,".owner.lock");
        Directory.CreateDirectory(_workspace);
        _workspaceLease = new(Path.Combine(_workspace, ".owner.lock"), FileMode.OpenOrCreate, FileAccess.ReadWrite, FileShare.None);
        _scheduler = new(); _journal = new(Path.Combine(_workspace, "logs", "downloads"));
        SourcePreferences = new DownloadSourcePreferences(Path.Combine(_workspace, "settings", "downloads.json"));
        _downloads = new(_scheduler, Path.Combine(_workspace, "shared", "cache"), eventSink: _journal, sourceSelector: new DownloadSourceSelector(()=>SourcePreferences.Mode, enableBmclApi:true), authorize: CurseForgeHeaders,redact:_curseForgeAccess.Redact);
        _backend = CreateBackend();
        Installations = new(new DynamicBackend(this), new JsonInstallationTaskStore(Path.Combine(_workspace, "downloads", "tasks")), GameLogs);
        Versions = new(_backend, _backend, new VersionResolver());
    }
    private IInstallationBackend CreateManagedBackend() => new ManagedInstanceInstallService(_workspace,
        root=>CreateBackend(root),_downloads,new JsonMetadataSource(_downloads,Path.Combine(_workspace,"shared","metadata")),
        ModpackInspector,HasCurseForgeAccess);
    private CmlLibInstallationBackend CreateBackend(string? instanceRoot = null)
    {
        var metadata = Path.Combine(_workspace, "shared", "metadata");
        return new(_downloads, new JsonMetadataSource(_downloads, metadata), new XmlMetadataSource(_downloads, metadata),
            instanceRoot ?? Path.Combine(_workspace, "instances"), metadata, Path.Combine(_workspace, "temp", "processors"), new JavaRuntimeService(),
            new JavaSelectionService(), string.IsNullOrWhiteSpace(ManualJava) ? [] : [ManualJava], GameLogs, string.IsNullOrWhiteSpace(ManualJava)?null:ManualJava);
    }
    public IReadOnlyList<DownloadEvent> DownloadEvents(Guid id) { var recent = _downloads.Events(id); return recent.Count != 0 ? recent : _journal.ReadTail(id); }
    public async Task<string> OpenInstalledAsync(InstallResult result, CancellationToken token)
    {
        // Ownership is established by a successful task in this workspace, never guessed from a path.
        if (!Installations.Records.Any(r => r.Result == result && r.Snapshot.State == DownloadTaskState.Completed))
            return "无法确认该实例的安装记录，请重新安装校验。";
        var engine = new ManagedLibraryEngine(this);
        if (!await Coordinator.ChangeEngineAsync(engine, token).ConfigureAwait(false)) return "请先结束当前游戏，再打开安装的实例。";
        var completed = Installations.Records.First(r => r.Result == result);
        Root = Path.Combine(_workspace,"instances"); IsMock = false;_managedLibrary=true;
        Coordinator.SelectInstance(completed.Recipe.InstanceKey.ToString("N"));SaveLibrarySelection();
        return "资料库已刷新。实例内容和设置保存在各自的 game 文件夹。";
    }
    public async ValueTask DisposeAsync()
    {
        await DrainLibraryPreferences().ConfigureAwait(false);
        await Installations.ShutdownAsync().ConfigureAwait(false);
        Accounts.Dispose(); _accountTransport.Dispose();
        _downloads.Dispose(); _journal.Dispose(); _scheduler.Dispose(); _workspaceLease.Dispose();
    }
    private sealed class DynamicBackend(LauncherComposition owner) : IInstallationBackend
    {
        public async Task<InstallResult> InstallAsync(InstallationRecipe recipe, IProgress<InstallProgress>? progress, CancellationToken token)
        {
            var root = Path.Combine(owner._workspace, "instances", recipe.InstanceKey.ToString("N"));
            var gate = owner._instanceAccess.For(root);
            progress?.Report(new("等待实例可用；游戏退出后继续安装"));
            await gate.WaitAsync(token).ConfigureAwait(false);
            try { return await owner.CreateManagedBackend().InstallAsync(recipe, progress, token).ConfigureAwait(false); }
            finally { gate.Release(); }
        }
    }
    private sealed class InstalledInstanceService(string id) : IInstanceService
    {
        public async Task<IReadOnlyList<InstanceInfo>> ScanAsync(string root, CancellationToken token) =>
            (await new CmlLibInstanceService().ScanAsync(root, token).ConfigureAwait(false)).Where(i => i.Id == id).ToArray();
    }
}
