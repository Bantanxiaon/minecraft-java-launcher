using System.ComponentModel;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Data;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Platform.Storage;
using Avalonia.VisualTree;
using Ursa.Controls.OverlayShared;
using Avalonia.Threading;
using SHLauncher.UI.Platform;
using SHLauncher.UI.ViewModels;
using SHLauncher.UI.Views.Pages;
using Ursa.Controls;
namespace SHLauncher.UI.Shell;

/// <summary>Routes existing application models into the shared presentation host.</summary>
public sealed class ApplicationPresentation
{
    private readonly SHLNWindow _window;
    public ShellViewModel Model { get; }
    private readonly AccountSurface _accounts;
    private readonly AccountCenter _accountCenter;
    private readonly HomeSurface _home;
    private readonly InstancesSurface _instances;
    private readonly ModsSurface _mods;
    private readonly DownloadsView _downloads;
    private readonly SettingsView _settings;
    private readonly DesktopTray _tray;
    private bool _exitRequested;
    private bool _opened;
    public void RequestExit(){_exitRequested=true;_tray.Restore();_window.Close();}
    public DesktopTray Tray=>_tray;
    private readonly MemoryPanel? _memoryPanel;
    private readonly Button[] _navigation;
    private readonly CancellationTokenSource _lifetime = new();
    private Task _initialization = Task.CompletedTask;
    private SHLauncher.UI.Components.ImportPreview? _importView;
    private bool _closing, _confirmed, _profileOpen, _importOpen;

    public ApplicationPresentation(SHLNWindow window, ShellViewModel model)
    {
        _window=window; Model=model; window.DataContext=model;
        window.Scene.Initialize(model.Backgrounds);
        model.Startup.Accounts.ProfileTextures=model.ProfileTextures;
        _accounts=new() { DataContext=model.Startup.Accounts };
        _accountCenter=new(model.Startup,model.ProfileTextures,window);
        window.FindControl<ContentControl>("AccountCenterHost")!.Content=_accountCenter;
        _home=new() { DataContext=model };
        _instances=new() { DataContext=model.Library };
        _mods=new(){DataContext=model.Mods};
        _downloads=new() { DataContext=model.Downloads };
        _settings=new() { DataContext=model.Settings };
        _tray=new(window,model,RequestExit);_settings.FindControl<ContentControl>("DesktopSettingsHost")!.Content=_tray.Settings;
        if(model.Memory is not null){_memoryPanel=new(model);_settings.FindControl<ContentControl>("MemorySettingsHost")!.Content=_memoryPanel;}
        _navigation=[Nav("主页", model.HomeCommand),Nav("实例",model.LibraryCommand),Nav("下载",model.DownloadsCommand),Nav("设置",model.SettingsCommand)];
        model.Settings.AttachDialogs(new FileDialogService(window));
        model.Import.AttachDialogs(new FileDialogService(window));
        model.Startup.ScreenChanged+=Render;
        model.PropertyChanged+=OnModelChanged;
        model.Settings.AppearanceChanged+=ApplyAppearance;
        model.Startup.Accounts.PanelChanged+=OnAccountPanel;
        model.Import.PropertyChanged+=OnImportChanged;
        DragDrop.SetAllowDrop(window,true);
        window.AddHandler(DragDrop.DragOverEvent,OnDragOver,RoutingStrategies.Tunnel,true);
        window.AddHandler(DragDrop.DropEvent,OnDrop,RoutingStrategies.Tunnel,true);
        window.Opened+=OnOpened;
        window.Closing+=OnClosing;
        window.Closed+=OnClosed;
        ApplyAppearance();
        window.SetNavigation([]);
        window.SetPage(new TextBlock { Text="正在读取账户与实例…", HorizontalAlignment=Avalonia.Layout.HorizontalAlignment.Center, VerticalAlignment=Avalonia.Layout.VerticalAlignment.Center },false);
    }
    private static Button Nav(string title, System.Windows.Input.ICommand command)
    { var button=new Button { Content=title,Command=command }; button.Classes.Add("nav");return button; }
    private void ApplyAppearance()
    {
        _window.SetReducedMotion(Model.Settings.ReducedMotion);
        _settings.FindControl<ThemeVariantScope>("LegacyTheme")!.RequestedThemeVariant=Avalonia.Styling.ThemeVariant.Dark;
        _downloads.FindControl<ThemeVariantScope>("LegacyTheme")!.RequestedThemeVariant=Avalonia.Styling.ThemeVariant.Dark;
    }
    private async void OnOpened(object? sender, EventArgs e)
    {
        if(_opened)return;_opened=true;
        _initialization=InitializeAsync(); await _initialization;
#if DEBUG
        if(!_closing) await Diagnostics.MasterAccountAudit.RunIfRequested(_window,this);
        if(!_closing) await Diagnostics.PhaseTwoAudit.RunIfRequested(_window,this);
#endif
    }
    private async Task InitializeAsync()
    {
        try
        {
            // Switch away from the historical demo engine before rendering any real page.
            if(Model.InitializeLibraryAsync is { } initialize) await initialize();
            else await Model.Launch.InitializeAsync();
            await Model.Downloads.InitializeAsync();
            await Model.Startup.InitializeAsync(_lifetime.Token);
        }
        catch(OperationCanceledException) when(_lifetime.IsCancellationRequested) { }
        catch(Exception error) { Model.Startup.Accounts.Report(error); if(!_closing) { _window.SetNavigation([]);_window.SetPage(_accounts); } }
    }
    private void OnModelChanged(object? sender, PropertyChangedEventArgs e) { if(e.PropertyName==nameof(ShellViewModel.Page))Render(); }
    private void Render()
    {
        if(_closing)return;
        _accountCenter.IsVisible=Model.Startup.Screen==RootScreen.MainShell;
        if(Model.Startup.Screen!=RootScreen.MainShell) { var manage=Model.Startup.Accounts.CanReturn;_accounts.SetWorkspace(manage);_window.SetWorkspace(manage);_window.SetNavigation([]);_window.SetPage(_accounts,!manage);return; }
        _window.SetWorkspace(!Model.IsHome);
        _window.SetNavigation(Model.IsHome ? [] : _navigation);
        var index=Model.Page switch { ShellPage.Library or ShellPage.Mods=>1,ShellPage.Downloads=>2,ShellPage.Settings=>3,_=>0 };
        for(var i=0;i<_navigation.Length;i++)_navigation[i].Classes.Set("selected",i==index);
        _window.SetPage(Model.Page==ShellPage.Mods?_mods:index switch { 1=>_instances,2=>_downloads,3=>_settings,_=>_home },false);
    }
    private async void OnAccountPanel()
    {
        var accounts=Model.Startup.Accounts;
        if(!accounts.IsProfiles) { Dispatcher.UIThread.Post(_accounts.FocusForm);return; }
        if(_profileOpen)return;
        _profileOpen=true;
        try
        {
            var list=new ListBox { ItemsSource=accounts.Profiles,MinWidth=280,MaxHeight=240 };
            list.ItemTemplate=new Avalonia.Controls.Templates.FuncDataTemplate<SHLauncher.Contracts.Accounts.AccountProfile>((p,_)=>new TextBlock {Text=p?.Name});
            list.Bind(ListBox.SelectedItemProperty,new Binding(nameof(accounts.SelectedProfile)) { Source=accounts,Mode=BindingMode.TwoWay });
            var content=new StackPanel { Spacing=12,Children={new TextBlock {Text="选择角色"},list} };
            var error=new TextBlock { TextWrapping=Avalonia.Media.TextWrapping.Wrap,MaxWidth=320 };
            error.Bind(TextBlock.TextProperty,new Binding(nameof(accounts.Message)) {Source=accounts});content.Children.Add(error);
            while(accounts.IsProfiles && Model.Startup.Screen==RootScreen.AccountGate && !_closing)
            {
                var result=await _window.ShowSheetAsync("选择角色",content);
                if(result!=DialogResult.OK) { accounts.BackCommand.Execute(null);break; }
                accounts.SelectProfileCommand.Execute(null);await accounts.SelectProfileCommand.DrainAsync();
            }
        }
        finally { _profileOpen=false; }
    }
    // Preserve the existing download page's import entry without a second overlay system.
    private async void OnImportChanged(object? sender, PropertyChangedEventArgs e)
    {
        if(e.PropertyName!=nameof(ModpackImportViewModel.IsOpen))return;
        if(!Model.Import.IsOpen) { _importView?.GetVisualAncestors().OfType<OverlayFeedbackElement>().FirstOrDefault()?.Close();return; }
        if(_importOpen)return;
        _importOpen=true;
        _importView=new() {DataContext=Model.Import};
        try { await _window.ShowDrawerAsync("导入整合包",_importView); }
        finally { _importView=null;if(Model.Import.IsOpen) Model.Import.CancelCommand.Execute(null);_importOpen=false; }
    }
    private static string[] DroppedPaths(DragEventArgs e)
    {
        var items=e.DataTransfer.TryGetFiles();
        if(items is null || items.Length!=1 || items[0] is not IStorageFile)return [];
        var path=items[0].TryGetLocalPath();
        return path is not null && path.EndsWith(".zip",StringComparison.OrdinalIgnoreCase) ? [path] : [];
    }
    private static string[] ModPaths(DragEventArgs e)=>e.DataTransfer.TryGetFiles()?.OfType<IStorageFile>().Select(f=>f.TryGetLocalPath()).OfType<string>().Where(p=>p.EndsWith(".jar",StringComparison.OrdinalIgnoreCase)).Take(1000).ToArray()??[];
    private void OnDragOver(object? sender,DragEventArgs e)
    { if(Model.Page==ShellPage.Mods){e.DragEffects=Model.Mods?.CanEdit==true&&ModPaths(e).Length>0?DragDropEffects.Copy:DragDropEffects.None;e.Handled=true;return;}e.DragEffects=DroppedPaths(e).Length==1&&!Model.Import.IsBusy ? DragDropEffects.Copy : DragDropEffects.None;e.Handled=true; }
    private async void OnDrop(object? sender,DragEventArgs e)
    { e.Handled=true;if(Model.Page==ShellPage.Mods&&Model.Mods is {} mods){await mods.InstallAsync(ModPaths(e));return;}await Model.Import.InspectAsync(DroppedPaths(e)); }
    private async void OnClosing(object? sender, WindowClosingEventArgs e)
    {
        if(_confirmed)return;
        e.Cancel=true;
        if(_closing)return;
        _closing=true;
        try{if(!_exitRequested&&await _tray.HandleCloseAsync()){_closing=false;return;}}
        catch(Exception error){_window.ShowToast(error.Message);_closing=false;return;}
        if(Model.Launch.IsRunning && Model.Settings.ConfirmRunningClose)
        {
            var result=await _window.ShowDialogAsync("结束游戏并关闭？",new TextBlock { Text="此操作会终止当前游戏。建议先在游戏内保存并退出。",MaxWidth=360,TextWrapping=Avalonia.Media.TextWrapping.Wrap });
            if(result!=DialogResult.OK){_exitRequested=false;_closing=false;return;}
        }
        _window.IsEnabled=false;
        _lifetime.Cancel();
        await _initialization;
        await _tray.DisposeAsync();
        if(Model.Mods is not null)await Model.Mods.DisposeAsync();
        if(_memoryPanel is not null)await _memoryPanel.DisposeAsync();
        await _accountCenter.DisposeAsync();
        _window.Scene.ClearSources();
        await Model.DisposeAsync();
        _confirmed=true;_window.Close();
    }
    private void OnClosed(object? sender, EventArgs e)
    {
        Model.Startup.ScreenChanged-=Render;Model.PropertyChanged-=OnModelChanged;
        Model.Settings.AppearanceChanged-=ApplyAppearance;Model.Startup.Accounts.PanelChanged-=OnAccountPanel;
        Model.Import.PropertyChanged-=OnImportChanged;
        _lifetime.Dispose();
    }
}
