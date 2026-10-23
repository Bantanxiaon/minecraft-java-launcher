using SHLauncher.Application.Install;
using SHLauncher.Contracts.Install;
using SHLauncher.Contracts.Platform;
using SHLauncher.UI.Presentation;
namespace SHLauncher.UI.ViewModels;
public sealed class ModpackImportViewModel : ObservableModel, IAsyncDisposable
{
    private readonly IInstallationWorkspace? _workspace;
    private readonly Action _showDownloads;
    private IFileDialogService? _dialogs;
    private readonly CancellationTokenSource _lifetime = new();
    private CancellationTokenSource? _inspection;
    private Task _work = Task.CompletedTask;
    private bool _open,_busy,_drag;
    private string _message="拖入一个 ZIP 整合包，先查看预览，再决定是否安装。",_displayName="";
    public ModpackPreview? Preview { get; private set; }
    public bool HasPreview=>Preview is not null;
    public bool IsOpen { get=>_open;private set=>Set(ref _open,value); }
    public bool IsBusy { get=>_busy;private set { if(Set(ref _busy,value)){ConfirmCommand.Refresh();ChooseCommand.Refresh();} } }
    public bool IsDragOver { get=>_drag;set=>Set(ref _drag,value); }
    public string Message { get=>_message;private set=>Set(ref _message,value); }
    public string DisplayName { get=>_displayName;set { if(Set(ref _displayName,value))ConfirmCommand.Refresh(); } }
    public string Summary => Preview is { } p ? $"{p.Format} · Minecraft {p.MinecraftVersion} · {p.Loader} {p.LoaderVersion}\n{p.RemoteFiles.Count} 个远程条目 · {p.LocalFiles.Count} 个包内文件 · {p.ArchiveSize/1048576d:0.0} MiB 原始文件" : "";
    public string Placement => Preview is null ? "" : "Mods、配置、资源包和光影将保留在新实例各自的 game 目录；确认前不会创建实例或解压文件。";
    public AsyncCommand ChooseCommand { get; }
    public DelegateCommand ConfirmCommand { get; }
    public DelegateCommand CancelCommand { get; }
    public ModpackImportViewModel(IInstallationWorkspace? workspace,Action showDownloads)
    {
        _workspace=workspace;_showDownloads=showDownloads;
        ChooseCommand=new(async()=>{if(_dialogs is null)return;var path=await _dialogs.PickModpackAsync(_lifetime.Token);if(path is not null)await InspectAsync([path]);},()=>!IsBusy && workspace is not null,e=>{Message="未能打开整合包："+e.GetType().Name;IsOpen=true;});
        ConfirmCommand=new(Confirm,()=>Preview is not null&&!IsBusy&&!string.IsNullOrWhiteSpace(DisplayName)&&DisplayName.Length<=64);
        CancelCommand=new(()=>{_inspection?.Cancel();Preview=null;IsOpen=false;Raise(nameof(Summary));Raise(nameof(HasPreview));ConfirmCommand.Refresh();});
    }
    public void AttachDialogs(IFileDialogService dialogs)=>_dialogs=dialogs;
    public Task InspectAsync(IReadOnlyList<string> paths)
    {
        if(IsBusy) { Message="正在检查前一个整合包，请稍候。";return Task.CompletedTask; }
        return _work=InspectCoreAsync(paths);
    }
    private async Task InspectCoreAsync(IReadOnlyList<string> paths)
    {
        IsDragOver=false;IsOpen=true;Preview=null;Raise(nameof(Summary));Raise(nameof(HasPreview));Raise(nameof(Placement));ConfirmCommand.Refresh();
        if(paths.Count!=1||!paths[0].EndsWith(".zip",StringComparison.OrdinalIgnoreCase)){Message="一次只接收一个 ZIP 整合包。";return;}
        if(_workspace is null){Message="安装服务尚未就绪。";return;}
        _inspection?.Dispose();_inspection=CancellationTokenSource.CreateLinkedTokenSource(_lifetime.Token);
        IsBusy=true;Message="正在只读检查格式、路径和压缩包完整性…";
        try
        {
            Preview=await _workspace.ModpackInspector.InspectAsync(paths[0],_inspection.Token);
            _inspection.Token.ThrowIfCancellationRequested();DisplayName=Preview.DisplayName;
            Message="请确认安装信息。远程文件需要 SH Launcher 自己的 CurseForge 访问配置；缺少访问条件时会明确停止，不会猜测下载地址。";
            Raise(nameof(Summary));Raise(nameof(HasPreview));Raise(nameof(Placement));
        }
        catch(OperationCanceledException){Preview=null;Message="检查已取消。";}
        catch(Exception e){Preview=null;Message="整合包无法导入："+e.Message;}
        finally{IsBusy=false;ConfirmCommand.Refresh();}
    }
    private void Confirm()
    {
        if(Preview is not { } p||_workspace is null)return;
        var recipe=new InstallationRecipe(Guid.NewGuid(),p.MinecraftVersion,p.Loader,p.LoaderVersion,DisplayName.Trim(),Guid.NewGuid(),p.ArchivePath,p.ArchiveSha256);
        if(_workspace.Installations.Enqueue(recipe)){IsOpen=false;Preview=null;_showDownloads();}
        else Message="任务暂时无法加入，请等待任务恢复完成或检查当前队列。";
    }
    public async ValueTask DisposeAsync()
    {
        await _lifetime.CancelAsync();await _work;
        _inspection?.Dispose();_lifetime.Dispose();
    }
}
