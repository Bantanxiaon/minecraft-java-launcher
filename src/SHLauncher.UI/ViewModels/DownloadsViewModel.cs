using System.Collections.ObjectModel;
using Avalonia.Threading;
using SHLauncher.Application.Install;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Install;
using SHLauncher.UI.Presentation;
namespace SHLauncher.UI.ViewModels;
public sealed partial class DownloadsViewModel : ObservableModel, IAsyncDisposable
{
    private readonly IInstallationWorkspace? _workspace;
    public event Action? InstallQueued;
    private string _search="";
    public string Search{get=>_search;set{if(Set(ref _search,value))Raise(nameof(VisibleVersions));}}
    public IReadOnlyList<MinecraftVersionInfo> VisibleVersions=>MinecraftVersions.Where(v=>v.Id.Contains(Search,StringComparison.OrdinalIgnoreCase)).ToArray();
    private readonly CancellationTokenSource _lifetime = new();
    private CancellationTokenSource? _selection;
    private IReadOnlyList<MinecraftVersionInfo> _all = [];
    private MinecraftVersionInfo? _minecraft;
    private LoaderVersionInfo? _loaderVersion;
    private LoaderType _loader;
    private int _filter;
    private bool _busy, _disposed;
    private bool _initializing=true;
    private bool _loadingLoaders;
    private int _refreshPending;
    private bool _installerExpanded=true;
    public bool IsInstallerExpanded {get=>_installerExpanded;set {if(Set(ref _installerExpanded,value))Raise(nameof(InstallerToggleLabel));}}
    public string InstallerToggleLabel=>IsInstallerExpanded?"收起选项":"选择版本";
    public DelegateCommand ToggleInstallerCommand {get;}
    private readonly List<Task> _pending = [];
    private readonly Dictionary<Guid, DownloadCardViewModel> _cards = new();
    private readonly HashSet<Guid> _opened = new();
    private Task Track(Task task) { _pending.RemoveAll(t => t.IsCompleted); _pending.Add(task); return task; }
    private string _message = "获取官方版本列表，开始一个新的实例。", _directoryName = "";
    public IReadOnlyList<MinecraftVersionInfo> MinecraftVersions { get; private set; } = [];
    public IReadOnlyList<LoaderVersionInfo> LoaderVersions { get; private set; } = [];
    public IReadOnlyList<LoaderType> LoaderTypes { get; } = Enum.GetValues<LoaderType>();
    public string[] Filters { get; } = ["正式版", "快照版", "全部"];
    public int Filter { get => _filter; set { if (Set(ref _filter, value)) ApplyFilter(); } }
    public MinecraftVersionInfo? Minecraft { get => _minecraft; set { if (Set(ref _minecraft, value)) { Raise(nameof(SelectionReason)); _ = Track(LoadLoadersAsync()); RefreshInstallState(); } } }
    public LoaderType Loader { get => _loader; set { if (Set(ref _loader, value)) { _ = Track(LoadLoadersAsync()); Raise(nameof(NeedsLoader));Raise(nameof(LoaderDescription));Raise(nameof(SelectionReason));RefreshInstallState(); } } }
    public bool NeedsLoader => Loader != LoaderType.Vanilla;
    public LoaderVersionInfo? LoaderVersion { get => _loaderVersion; set { if (Set(ref _loaderVersion, value)) { Raise(nameof(SelectionReason)); RefreshInstallState(); } } }
    public string DirectoryName { get => _directoryName; set { if (Set(ref _directoryName, value)) RefreshInstallState(); } }
    public string Message { get => _message; private set => Set(ref _message, value); }
    public string RefreshLabel => Busy?"正在刷新…":"刷新版本";
    public string LoaderDescription => Loader switch {
        LoaderType.Vanilla=>"原版 Minecraft，不安装额外加载器。",
        LoaderType.Fabric=>"将安装 Fabric Loader，并创建独立实例。",
        LoaderType.Forge=>"将安装 Forge，并创建独立实例。",
        LoaderType.NeoForge=>"将安装 NeoForge，并创建独立实例。",
        LoaderType.Quilt=>"将安装 Quilt Loader，并创建独立实例。",
        _=>"请选择加载器。"};
    public string SelectionReason => Minecraft?.Kind is MinecraftVersionKind.Snapshot ? "当前仅支持安装正式版。快照仍可浏览，请切回正式版后安装。" : (LoaderVersion is not null ? (LoaderVersion.IsRecommended ? "来自官方推荐版本。" : "已获取与当前 Minecraft 对应的稳定版本。") : null) ?? (NeedsLoader ? "请选择当前 Minecraft 对应的稳定 Loader。" : "Vanilla 原版 · 不安装 Loader。");
    public bool Busy { get => _busy; private set { if (Set(ref _busy, value)) { RefreshCommand.Refresh(); Raise(nameof(RefreshLabel));RefreshInstallState(); } } }
    public ObservableCollection<DownloadCardViewModel> Tasks { get; } = [];
    public System.Windows.Input.ICommand? ImportCommand {get;set;}
    public bool IsEmpty => Tasks.Count==0;
    public int ActiveCount => Tasks.Count(t=>t.IsActive);
    public string TaskSummary => ActiveCount>0 ? ActiveCount+" 个任务进行中" : Tasks.Count==0 ? "暂无下载任务" : Tasks.Count+" 个任务";

    public AsyncCommand RefreshCommand { get; }
    public DelegateCommand InstallCommand { get; }
    public DownloadsViewModel(IInstallationWorkspace? workspace)
    {
        _workspace = workspace;
        ToggleInstallerCommand=new(()=>IsInstallerExpanded=!IsInstallerExpanded);
        ResolutionRetryCommand=new(async()=>{
            if(Minecraft is null || _catalogFailed)await RefreshAsync();
            else await Track(LoadLoadersAsync());
        },()=>!Busy&&!_loadingLoaders&&!_disposed&&_workspace is not null,e=>SetResolutionProblem("暂时无法获取版本信息。",e,"重试版本信息"));
        RefreshCommand = new(RefreshAsync, () => !Busy && !_disposed && workspace is not null, e => SetResolutionProblem("暂时无法获取版本列表。",e,"Minecraft 官方目录"));
        InstallCommand = new(Install, CanInstall);
        if (workspace is not null) workspace.Installations.Changed += QueueChanged;
    }
    public async Task InitializeAsync()
    {
        if (_workspace is null) return;
        try { await _workspace.Installations.InitializeAsync(_lifetime.Token);foreach(var record in _workspace.Installations.Records.Where(r=>r.Result is not null))_opened.Add(record.Recipe.Id);_initializing=false;if(!_disposed)UpdateTasks(); }
        catch(OperationCanceledException)when(_lifetime.IsCancellationRequested){ }
    }
    public Task RefreshAsync() => Track(RefreshCoreAsync());
    private async Task RefreshCoreAsync()
    {
        if (_workspace is null || Busy) return;
        Busy = true; Message = "正在获取官方 Minecraft 版本…";
        try { ClearResolutionProblem();_all = await _workspace.Versions.FetchMinecraftAsync(_lifetime.Token); ApplyFilter(); Message = "已更新官方版本。当前支持安装正式版。"; }
        catch (OperationCanceledException) { }
        catch (Exception e) { SetResolutionProblem("暂时无法获取版本列表。",e,"Minecraft 官方目录");Message="请稍后重试。"; }
        finally { Busy = false; }
    }
    private void ApplyFilter()
    {
        MinecraftVersions = _workspace?.Versions.Filter(_all, Filter switch { 0 => MinecraftVersionKind.Release, 1 => MinecraftVersionKind.Snapshot, _ => null }) ?? [];
        Raise(nameof(MinecraftVersions));Raise(nameof(VisibleVersions)); Minecraft = MinecraftVersions.FirstOrDefault(v => v.OfficialLatestRelease) ?? MinecraftVersions.FirstOrDefault();
    }
    private async Task LoadLoadersAsync()
    {
        _selection?.Cancel(); _selection?.Dispose(); _selection = CancellationTokenSource.CreateLinkedTokenSource(_lifetime.Token);
        var token = _selection.Token; var minecraft = Minecraft; var loader = Loader;
        _loadingLoaders=false;ClearResolutionProblem();
        LoaderVersions = []; LoaderVersion = null; Raise(nameof(LoaderVersions));Raise(nameof(SelectionReason)); RefreshInstallState();
        if (_workspace is null || minecraft is null || loader == LoaderType.Vanilla) return;
        _loadingLoaders=true;RefreshInstallState();
        Message = "正在核实 " + loader + " 对 " + minecraft.Id + " 的支持…";
        try
        {
            var list = await _workspace.Versions.FetchLoadersAsync(loader, minecraft.Id, token);
            if (token.IsCancellationRequested || _disposed) return;
            LoaderVersions = list; Raise(nameof(LoaderVersions)); LoaderVersion = _workspace.Versions.Preferred(list);
            Message = list.Count == 0 ? "当前 Minecraft 没有可用的稳定 " + loader + " 版本，请更换版本或 Loader。" : "已取得对应版本。选择依据显示在下方。";
        }
        catch (OperationCanceledException) { }
        catch (Exception e) { if (!token.IsCancellationRequested) {
            SetResolutionProblem("暂时无法获取兼容版本。",e,loader+" · Minecraft "+minecraft.Id);
            Message="不会自动猜测版本，以免创建无法启动的实例。";
        } }
        finally {if(!token.IsCancellationRequested){_loadingLoaders=false;RefreshInstallState();}}
    }
    private bool CanInstall() => InstallBlocker.Length==0;
    private void Install()
    {
        if (!CanInstall()) return;
        var recipe = new InstallationRecipe(Guid.NewGuid(), Minecraft!.Id, Loader, LoaderVersion?.Version ?? "", DirectoryName.Trim());
        var queued=_workspace!.Installations.Enqueue(recipe);
        Message = queued ? "已加入下载任务。" : "任务未加入：请检查任务是否正在恢复或已达到队列上限。";
        if(queued)InstallQueued?.Invoke();
    }
    private void QueueChanged()
    {
        if (Interlocked.Exchange(ref _refreshPending, 1) != 0) return;
        Dispatcher.UIThread.Post(() => { Interlocked.Exchange(ref _refreshPending, 0); if (!_disposed) UpdateTasks(); });
    }
    private void UpdateTasks()
    {
        if(_initializing)return;
        var records = _workspace?.Installations.Records ?? [];
        _opened.IntersectWith(records.Select(r=>r.Recipe.Id));
        if(records.Any(r=>!_cards.ContainsKey(r.Recipe.Id)))IsInstallerExpanded=false;
        foreach(var old in _cards.Keys.Except(records.Select(r=>r.Recipe.Id)).ToArray()){Tasks.Remove(_cards[old]);_cards.Remove(old);}
        for(var index=0;index<records.Count;index++)
        {
            var record=records[index];
            if(!_cards.TryGetValue(record.Recipe.Id,out var card)){card=new(record,_workspace!,text=>Message=text);_cards.Add(record.Recipe.Id,card);Tasks.Insert(index,card);}
            else {card.Update(record);var position=Tasks.IndexOf(card);if(position!=index)Tasks.Move(position,index);}
        }
        var ready = records.FirstOrDefault(r => r.Result is not null && r.Snapshot.State == DownloadTaskState.Completed && !_opened.Contains(r.Recipe.Id));
        if (ready is not null)
        {
            foreach (var done in records.Where(r=>r.Result is not null)) _opened.Add(done.Recipe.Id);
            _ = Track(OpenReadyAsync(ready.Result!));
        }
        if (_workspace?.Installations.PersistenceProblem is { } problem) Message = problem;
        Raise(nameof(IsEmpty));Raise(nameof(ActiveCount));Raise(nameof(TaskSummary));
    }
    private async Task OpenReadyAsync(InstallResult result) { if (_workspace is not null) Message = await _workspace.OpenInstalledAsync(result,_lifetime.Token); }
    public async ValueTask DisposeAsync()
    {
        _disposed = true;
        if (_workspace is not null) _workspace.Installations.Changed -= QueueChanged;
        await _lifetime.CancelAsync();
        await Task.WhenAll(_pending.ToArray());
        await Task.WhenAll(RefreshCommand.DrainAsync(),ResolutionRetryCommand.DrainAsync());
        if (_workspace is not null) await _workspace.DisposeAsync();
        _selection?.Dispose(); _lifetime.Dispose();
    }
}
