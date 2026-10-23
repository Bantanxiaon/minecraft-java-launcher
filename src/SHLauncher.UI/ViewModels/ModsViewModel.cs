using Avalonia.Threading;
using SHLauncher.Contracts.Content;
using SHLauncher.Contracts.Engine;
using SHLauncher.UI.Presentation;
namespace SHLauncher.UI.ViewModels;
public sealed class ModRow:ObservableModel
{
    public LocalMod Mod{get;}public string Name=>Mod.Name;public string Summary=>$"{Mod.Version} · {Mod.Loader} · {Mod.FileName}";
    public string State=>Mod.Enabled?"启用":"禁用";public string? Problem=>Mod.Problem;public bool HasProblem=>!string.IsNullOrEmpty(Problem);
    private bool _selected;public bool Selected{get=>_selected;set{if(Set(ref _selected,value))Changed?.Invoke();}}public event Action? Changed;
    public ModRow(LocalMod mod)=>Mod=mod;
}
public sealed class ModsViewModel:ObservableModel,IAsyncDisposable
{
    private readonly LaunchViewModel _launch;private readonly IInstanceModService _service;private readonly CancellationTokenSource _lifetime=new();
    private IDisposable? _watch;private InstanceInfo? _instance;private IReadOnlyList<ModRow> _rows=[];private string _search="",_message="";private int _filter;private bool _busy,_active;private Task _pending=Task.CompletedTask;private bool _refreshAgain,_forceAgain;private Task _operation=Task.CompletedTask;
    public IReadOnlyList<ModRow> Rows=>_rows.Where(r=>(Filter==0||Filter==1&&r.Mod.Enabled||Filter==2&&!r.Mod.Enabled)&&(r.Name.Contains(Search,StringComparison.OrdinalIgnoreCase)||r.Mod.Id.Contains(Search,StringComparison.OrdinalIgnoreCase)||r.Mod.FileName.Contains(Search,StringComparison.OrdinalIgnoreCase))).ToArray();
    public string Search{get=>_search;set{if(Set(ref _search,value))Raise(nameof(Rows));}}public int Filter{get=>_filter;set{if(Set(ref _filter,value))Raise(nameof(Rows));}}
    public int ReleaseUnusedResources()
    {
        if(Busy||_active)return 0;var count=_service.ClearMetadataCache();foreach(var row in _rows)row.Changed-=SelectionChanged;_rows=[];Raise(nameof(Rows));Raise(nameof(Counts));SelectionChanged();Online?.ReleaseUnusedResources();return count;
    }
    public OnlineModsViewModel? Online{get;}
    public void PrepareOnline(){if(!CanEdit||_instance is null)throw new IOException("请选择可编辑的独立实例。");Online?.Activate(_instance);}
    public string[] Filters{get;}=["全部","启用","禁用"];
    public string Title=>"Mod · "+(_instance?.Name??"未选择实例");public string Counts=>$"{_rows.Count} 个 Mod · {_rows.Count(r=>r.Mod.Enabled)} 个启用";
    public int SelectedCount=>_rows.Count(r=>r.Selected);public bool HasSelection=>SelectedCount>0;
    public bool HasMessage=>!string.IsNullOrEmpty(Message);
    public string Message{get=>_message;private set{if(Set(ref _message,value))Raise(nameof(HasMessage));}}
    public bool Busy{get=>_busy;private set{Set(ref _busy,value);Raise(nameof(CanEdit));}}
    public bool CanEdit=>!_lifetime.IsCancellationRequested&&!Busy&&_launch.CanSelect&&_instance is not null&&_service.CanManage(_instance);
    public DelegateCommand RefreshCommand{get;}public DelegateCommand ForceRefreshCommand{get;}public DelegateCommand SelectAllCommand{get;}public DelegateCommand OpenFolderCommand{get;}
    public AsyncCommand EnableCommand{get;}public AsyncCommand DisableCommand{get;}
    public ModsViewModel(LaunchViewModel launch,IInstanceModService service,OnlineModsViewModel? online=null)
    {
        _launch=launch;_service=service;Online=online;if(Online is not null)Online.Installed+=OnlineInstalled;_launch.Refreshed+=LaunchChanged;RefreshCommand=new(()=>Refresh(false));ForceRefreshCommand=new(()=>Refresh(true));SelectAllCommand=new(()=>{var rows=Rows;var select=rows.Any(r=>!r.Selected);foreach(var row in rows)row.Selected=select;});
        OpenFolderCommand=new(()=>{try{if(_instance is not null)_service.OpenFolder(_instance);}catch(Exception e){Message=e.Message;}});
        EnableCommand=new(()=>Mutate((i,f,t)=>_service.SetEnabledAsync(i,f,true,t)),()=>CanEdit,e=>Message=e.Message);
        DisableCommand=new(()=>Mutate((i,f,t)=>_service.SetEnabledAsync(i,f,false,t)),()=>CanEdit,e=>Message=e.Message);
    }
    private void OnlineInstalled()=>Refresh(true);
    private void LaunchChanged(){Raise(nameof(CanEdit));EnableCommand.Refresh();DisableCommand.Refresh();if(_active&&_instance!=_launch.Selected)Activate();}
    public void Activate(){_active=true;_instance=_launch.Selected;Raise(nameof(Title));Refresh(false);_watch?.Dispose();_watch=null;if(_instance is not null)try{_watch=_service.Watch(_instance,()=>Dispatcher.UIThread.Post(()=>{if(_active)Refresh(false);}));}catch(Exception e){Message=e.Message;}}
    public void Deactivate(){_active=false;_watch?.Dispose();_watch=null;}
    private void SelectionChanged(){Raise(nameof(SelectedCount));Raise(nameof(HasSelection));}
    private void Refresh(bool force){if(_lifetime.IsCancellationRequested)return;if(Busy){_refreshAgain=true;_forceAgain|=force;return;}_pending=RefreshAsync(force);}
    private async Task RefreshAsync(bool force)
    {
        if(_instance is null){Message="请先选择实例。";return;}Busy=true;var instance=_instance;
        try{var rows=await _service.ScanAsync(instance,force,_lifetime.Token);if(_instance!=instance||_lifetime.IsCancellationRequested)return;foreach(var row in _rows)row.Changed-=SelectionChanged;_rows=rows.Select(m=>new ModRow(m)).ToArray();foreach(var row in _rows)row.Changed+=SelectionChanged;Raise(nameof(Rows));Raise(nameof(Counts));SelectionChanged();if(!_service.CanManage(_instance))Message="此目录为只读连接。请导入为独立实例后管理 Mod。";else if(Message=="请先选择实例。"||Message.StartsWith("此目录为只读连接"))Message="";}
        catch(OperationCanceledException){}catch(Exception e){Message=e.Message;}
        finally{Busy=false;EnableCommand.Refresh();DisableCommand.Refresh();if(_refreshAgain&&!_lifetime.IsCancellationRequested){_refreshAgain=false;var forced=_forceAgain;_forceAgain=false;Refresh(forced);}}
    }
    private string[] Selected()=>_rows.Where(r=>r.Selected).Select(r=>r.Mod.FileName).ToArray();
    private async Task Mutate(Func<InstanceInfo,IReadOnlyList<string>,CancellationToken,Task<IReadOnlyList<ModOperationResult>>> operation)
    {
        if(!CanEdit||_instance is null)return;Busy=true;
        try{var results=await operation(_instance,Selected(),_lifetime.Token);Message=Summary(results);}
        finally{Busy=false;Refresh(true);}
    }
    public Task RecycleSelectedAsync()=>!CanEdit?Task.CompletedTask:(_operation=Mutate(_service.RecycleAsync));
    public Task InstallAsync(IReadOnlyList<string> files)=>!CanEdit?Task.CompletedTask:(_operation=InstallCoreAsync(files));
    private async Task InstallCoreAsync(IReadOnlyList<string> files)
    {
        if(!CanEdit||_instance is null){Message="请先选择可编辑的独立实例。";return;}Busy=true;
        try{Message=Summary(await _service.InstallLocalAsync(_instance,files,_lifetime.Token));}catch(Exception e){Message=e.Message;}finally{Busy=false;Refresh(true);}
    }
    private static string Summary(IReadOnlyList<ModOperationResult> results)=>$"完成 {results.Count(r=>r.Succeeded)} / {results.Count}"+string.Join("",results.Where(r=>!r.Succeeded).Take(3).Select(r=>"\n"+r.FileName+"："+r.Message));
    public async ValueTask DisposeAsync(){if(Online is not null){Online.Installed-=OnlineInstalled;await Online.DisposeAsync();}_launch.Refreshed-=LaunchChanged;Deactivate();_lifetime.Cancel();await _operation;await _pending;await Task.WhenAll(EnableCommand.DrainAsync(),DisableCommand.DrainAsync());foreach(var row in _rows)row.Changed-=SelectionChanged;_lifetime.Dispose();}
}
