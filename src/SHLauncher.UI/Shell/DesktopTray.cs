using Avalonia;
using Avalonia.Controls;
using Avalonia.Platform;
using SHLauncher.Contracts.Runtime;
using SHLauncher.UI.ViewModels;
using Ursa.Controls;
namespace SHLauncher.UI.Shell;
public sealed class DesktopTray:IAsyncDisposable
{
    private readonly SHLNWindow _window;private readonly ShellViewModel _model;private readonly TrayIcon _icon;private readonly NativeMenuItem _launch;private readonly Action _exit;private bool _disposed;private Task _cleanup=Task.CompletedTask;
    private DesktopPreferences _preferences;public Control Settings{get;}
    public bool Available=>_icon.NativeMenuExporter is not null;
    public DesktopTray(SHLNWindow window,ShellViewModel model,Action exit)
    {
        _window=window;_model=model;_exit=exit;
        try{_preferences=model.DesktopPreferences?.Read()??new();}catch(Exception error){_preferences=new();window.ShowToast(error.Message);}
        NativeMenuItem Item(string text,Action action){var item=new NativeMenuItem(text);item.Click+=(_,_)=>action();return item;}
        var menu=new NativeMenu();menu.Items.Add(Item("打开 SHLN",Restore));
        _launch=Item("启动游戏",()=>{Restore();if(model.Launch.PresentationCanStart)model.Launch.StartCommand.Execute(null);});menu.Items.Add(_launch);
        menu.Items.Add(Item("实例",()=>{Restore();model.LibraryCommand.Execute(null);}));menu.Items.Add(Item("下载",()=>{Restore();model.DownloadsCommand.Execute(null);}));
        menu.Items.Add(Item("清理未使用资源",()=>{Restore();_=CleanupAsync();}));menu.Items.Add(new NativeMenuItemSeparator());menu.Items.Add(Item("退出 SHLN",exit));
        using var stream=AssetLoader.Open(new Uri("avares://SHLauncher.UI/Assets/Icons/shln.png"));
        _icon=new TrayIcon{Icon=new WindowIcon(stream),ToolTipText="SHLN",Menu=menu,IsVisible=true};_icon.Clicked+=Clicked;
        TrayIcon.SetIcons(Avalonia.Application.Current!,new TrayIcons{_icon});model.Launch.Refreshed+=Refresh;model.Startup.Accounts.PropertyChanged+=AccountChanged;Refresh();
        var mode=new ComboBox{ItemsSource=new[]{"每次询问","退出启动器","最小化到托盘"},SelectedIndex=(int)_preferences.CloseAction,MinWidth=180};
        var status=new TextBlock{TextWrapping=Avalonia.Media.TextWrapping.Wrap};var save=new Button{Content="保存关闭方式"};
        save.Click+=(_,_)=>{try{var action=(DesktopCloseAction)mode.SelectedIndex;if(action==DesktopCloseAction.Tray&&!Available)throw new IOException("当前系统未提供可用托盘。");_preferences=_preferences with{CloseAction=action};model.DesktopPreferences?.Save(_preferences);status.Text="关闭方式已保存。";}catch(Exception error){status.Text=error.Message;}};
        var clean=new Button{Content="清理未使用资源"};clean.Click+=(_,_)=>_=CleanupAsync();
        Settings=new StackPanel{Spacing=10,Children={new TextBlock{Text="窗口与后台",FontSize=18},mode,save,clean,status}};
    }
    private void AccountChanged(object? sender,System.ComponentModel.PropertyChangedEventArgs e)=>Refresh();
    private void Clicked(object? sender,EventArgs e)=>Restore();
    private void Refresh(){_launch.IsEnabled=_model.Startup.Accounts.HasActive&&_model.Launch.PresentationCanStart;_launch.Header=_model.Launch.IsRunning?"游戏运行中":"启动当前实例";}
    public void Restore(){if(_disposed)return;_window.ShowInTaskbar=true;if(!_window.IsVisible)_window.Show();if(_window.WindowState==WindowState.Minimized)_window.WindowState=WindowState.Normal;_window.Activate();}
    public async Task<bool> HandleCloseAsync()
    {
        var action=_preferences.CloseAction;
        if(action==DesktopCloseAction.Ask)
        {
            var choices=new ComboBox{ItemsSource=new[]{"最小化到托盘，任务继续运行","退出启动器"},SelectedIndex=Available?0:1,MinWidth=300};var remember=new CheckBox{Content="记住此选择"};
            var result=await _window.ShowDialogAsync("关闭窗口",new StackPanel{Spacing=12,Children={new TextBlock{Text="留在托盘后，可从系统托盘重新打开 SHLN。",TextWrapping=Avalonia.Media.TextWrapping.Wrap},choices,remember}});
            if(result!=DialogResult.OK)return true;action=choices.SelectedIndex==0?DesktopCloseAction.Tray:DesktopCloseAction.Exit;
            if(remember.IsChecked==true){_preferences=_preferences with{CloseAction=action};_model.DesktopPreferences?.Save(_preferences);}
        }
        if(action==DesktopCloseAction.Exit)return false;
        if(!Available){_window.ShowToast("系统托盘不可用，窗口保持打开。请改选退出启动器。");return true;}
        if(!_preferences.TrayNoticeSeen)
        {
            if(await _window.ShowDialogAsync("SHLN 将留在托盘",new TextBlock{Text="窗口隐藏后，下载和游戏会继续运行。点击系统托盘中的 SHLN 图标可重新打开；托盘菜单可以退出启动器。",TextWrapping=Avalonia.Media.TextWrapping.Wrap,MaxWidth=400})!=DialogResult.OK)return true;
            _preferences=_preferences with{TrayNoticeSeen=true};_model.DesktopPreferences?.Save(_preferences);
        }
        _window.Hide();_window.ShowInTaskbar=false;return true;
    }
    public Task CleanupAsync()=>_disposed?Task.CompletedTask:!_cleanup.IsCompleted?_cleanup:(_cleanup=CleanupCoreAsync());
    private async Task CleanupCoreAsync()
    {
        var before=_model.ProcessMemory?.WorkingSetBytes();var count=_model.Mods?.ReleaseUnusedResources()??0;var decoder=_model.Backgrounds.ReleasePausedPlayer();
        await Task.Delay(150);if(_disposed)return;var after=_model.ProcessMemory?.WorkingSetBytes();
        var measurement=before is {} b&&after is {} a?$"当前内存 {a/1048576d:0.0} MiB，变化 {(a-b)/1048576d:+0.0;-0.0;0.0} MiB。":"当前平台无法读取进程内存。";
        _window.ShowToast($"已清理 {count} 项闲置缓存"+(decoder?"和暂停的背景资源。":"。")+measurement);
    }
    public async ValueTask DisposeAsync(){_disposed=true;_model.Launch.Refreshed-=Refresh;_model.Startup.Accounts.PropertyChanged-=AccountChanged;_icon.Clicked-=Clicked;_icon.IsVisible=false;TrayIcon.SetIcons(Avalonia.Application.Current!,null);_icon.Dispose();await _cleanup;}
}
