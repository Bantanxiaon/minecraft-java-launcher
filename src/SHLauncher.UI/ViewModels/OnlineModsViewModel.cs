using SHLauncher.Contracts.Content;
using SHLauncher.Contracts.Engine;
using SHLauncher.UI.Presentation;
namespace SHLauncher.UI.ViewModels;
public sealed class OnlineModsViewModel:ObservableModel,IAsyncDisposable
{
    private readonly IReadOnlyList<IOnlineModProvider> _providers;private readonly IOnlineModService _service;private readonly CancellationTokenSource _lifetime=new();private CancellationTokenSource? _operation;
    private InstanceInfo? _instance;private string _query="",_message="",_planText="";private int _provider;private bool _busy;private ContentSearchResult? _project;private OnlineModFile? _version;private ModInstallPreview? _preview;
    private IReadOnlyList<ContentSearchResult> _projects=[];private IReadOnlyList<OnlineModFile> _versions=[];
    public string[] Providers=>_providers.Select(p=>p.Id).ToArray();
    public int Provider{get=>_provider;set{if(!Busy&&Set(ref _provider,value)){Projects=[];Versions=[];Project=null;Version=null;ClearPreview();}}}
    public string Query{get=>_query;set=>Set(ref _query,value);}public string Context=>_instance is null?"未选择实例":$"{_instance.Name} · {_instance.GameVersion} · {_instance.Loader}";
    public string Message{get=>_message;private set=>Set(ref _message,value);}public string PlanText{get=>_planText;private set=>Set(ref _planText,value);}
    public bool Busy{get=>_busy;private set{Set(ref _busy,value);Raise(nameof(CanInstall));SearchCommand.Refresh();VersionsCommand.Refresh();PreviewCommand.Refresh();InstallCommand.Refresh();RecoverCommand.Refresh();}}
    public bool CanInstall=>!Busy&&_preview?.CanInstall==true;
    public IReadOnlyList<ContentSearchResult> Projects{get=>_projects;private set=>Set(ref _projects,value);}public IReadOnlyList<OnlineModFile> Versions{get=>_versions;private set=>Set(ref _versions,value);}
    public ContentSearchResult? Project{get=>_project;set{if(Set(ref _project,value)){Versions=[];Version=null;ClearPreview();VersionsCommand.Refresh();}}}
    public OnlineModFile? Version{get=>_version;set{if(Set(ref _version,value)){ClearPreview();PreviewCommand.Refresh();}}}
    public AsyncCommand RecoverCommand{get;}public AsyncCommand SearchCommand{get;}public AsyncCommand VersionsCommand{get;}public AsyncCommand PreviewCommand{get;}public AsyncCommand InstallCommand{get;}public DelegateCommand CancelCommand{get;}
    public event Action? Installed;
    public OnlineModsViewModel(IReadOnlyList<IOnlineModProvider> providers,IOnlineModService service)
    {
        _providers=providers;_service=service;
        SearchCommand=new(()=>Run(async token=>{var instance=Required();Projects=await _providers[Provider].SearchAsync(new(Query,ContentType.Mod,instance.GameVersion,instance.Loader),token);Message=$"找到 {Projects.Count} 项（最多显示 40 项）";}),()=>!Busy,e=>Message=e.Message);
        VersionsCommand=new(()=>Run(async token=>{var instance=Required();var project=Project??throw new IOException("请选择项目。");Versions=await _providers.Single(p=>p.Id==project.ProviderId).VersionsAsync(project.Id,new(instance.GameVersion,instance.Loader,null),token);Version=Versions.FirstOrDefault();Message=$"{Versions.Count} 个匹配版本";}),()=>!Busy&&Project is not null,e=>Message=e.Message);
        PreviewCommand=new(()=>Run(async token=>{_preview=await _service.PreviewAsync(Required(),Version??throw new IOException("请选择版本。"),token);PlanText=string.Join("\n",_preview.Targets.Select(t=>$"{(t.AlreadyInstalled?"已安装":t.PreviousFile is null?"新增":"更新")} · {t.Artifact.FileName} · {t.Artifact.Size/1048576d:0.00} MiB\n{t.Destination}"))+"\n"+string.Join("\n",_preview.Problems.Concat(_preview.Dependencies.Problems.Select(p=>p.Reason)));Message=_preview.CanInstall?"请核对依赖和目标文件后安装。":"安装计划存在问题，尚未修改实例。";}),()=>!Busy&&Version is not null,e=>Message=e.Message);
        InstallCommand=new(()=>Run(async token=>{var preview=_preview??throw new IOException("请先预览安装计划。");_preview=null;await _service.InstallAsync(Required(),preview,new Progress<SHLauncher.Contracts.Downloads.DownloadTaskSnapshot>(p=>{if(_operation?.Token==token)Message=p.Problem??$"{p.Stage??p.State.ToString()} · {p.Progress.CompletedBytes/1048576d:0.0} MiB";}),token);Message="安装完成，旧版本备份已保留。";Installed?.Invoke();}),()=>CanInstall,e=>Message=e.Message);
        RecoverCommand=new(()=>Run(async token=>{ClearPreview();await _service.RecoverAsync(Required(),token);Message="本地恢复检查完成，可以重新管理或启动实例。";Installed?.Invoke();}),()=>!Busy,e=>Message=e.Message);
        CancelCommand=new(()=>_operation?.Cancel());
    }
    public void ReleaseUnusedResources(){if(Busy)return;Projects=[];Versions=[];Project=null;Version=null;ClearPreview();}
    public void Activate(InstanceInfo instance){if(Busy)throw new IOException("请等待当前内容任务完成或取消。");_instance=instance;Projects=[];Versions=[];Project=null;Version=null;Message="搜索结果将匹配当前实例的版本与 Loader。";ClearPreview();Raise(nameof(Context));}
    private InstanceInfo Required()=>_instance??throw new IOException("请先选择实例。");
    private void ClearPreview(){_preview=null;PlanText="";Raise(nameof(CanInstall));InstallCommand?.Refresh();}
    private async Task Run(Func<CancellationToken,Task> work)
    {
        if(Busy)return;using var operation=CancellationTokenSource.CreateLinkedTokenSource(_lifetime.Token);_operation=operation;Busy=true;
        try{await work(operation.Token);}catch(OperationCanceledException){Message="操作已取消，未完成文件保留在暂存区。";}finally{_operation=null;Busy=false;}
    }
    public async ValueTask DisposeAsync(){_lifetime.Cancel();await Task.WhenAll(SearchCommand.DrainAsync(),VersionsCommand.DrainAsync(),PreviewCommand.DrainAsync(),InstallCommand.DrainAsync(),RecoverCommand.DrainAsync());_lifetime.Dispose();}
}
