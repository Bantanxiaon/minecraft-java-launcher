using SHLauncher.UI.Presentation;

namespace SHLauncher.UI.ViewModels;

public enum ShellPage { Home, Library, Settings, Downloads, Mods }

public sealed class ShellViewModel : ObservableModel, IAsyncDisposable
{
    public SHLauncher.Contracts.Runtime.IDesktopPreferences? DesktopPreferences{get;set;}
    public SHLauncher.Contracts.Runtime.IProcessMemory? ProcessMemory{get;set;}

    private ShellPage _page;
    public ModsViewModel? Mods {get;set;}
    public DelegateCommand ModsCommand{get;}
    public SHLauncher.Contracts.Runtime.IGameMemoryService? Memory {get;set;}
    public Func<SHLauncher.Contracts.Engine.InstanceInfo,CancellationToken,Task<SHLauncher.Contracts.Runtime.JavaRuntime?>>? MemoryJava {get;set;}
    public SHLauncher.Contracts.Accounts.IProfileTextureService? ProfileTextures {get;set;}
    public Func<Task>? InitializeLibraryAsync { get; set; }
    public BackgroundSession Backgrounds { get; }
    public ModpackImportViewModel Import { get; }
    public DownloadsViewModel Downloads { get; }
    public HomeViewModel Home { get; }
    public LibraryViewModel Library { get; }
    public SettingsViewModel Settings { get; }
    public LaunchViewModel Launch { get; }
    public ShellPage Page { get => _page; set { if (Set(ref _page, value)) { Raise(nameof(CurrentPage)); Raise(nameof(IsHome)); Raise(nameof(IsLibrary)); Raise(nameof(IsSettings)); Raise(nameof(IsDownloads)); } } }
    public object CurrentPage => Page switch { ShellPage.Downloads => Downloads, ShellPage.Library => Library, ShellPage.Settings => Settings, _ => Home };
    public bool IsDownloads => Page == ShellPage.Downloads;
    public DelegateCommand DownloadsCommand { get; }
    public bool IsHome => Page == ShellPage.Home;
    public bool IsLibrary => Page == ShellPage.Library;
    public bool IsSettings => Page == ShellPage.Settings;
    public DelegateCommand HomeCommand { get; }
    public DelegateCommand LibraryCommand { get; }
    public DelegateCommand SettingsCommand { get; }
    public string AccountMode => Settings.ConfirmRunningClose ? "本地离线资料" : "演示资料";
    public string Title => "SHLN";
    public StartupViewModel Startup {get;}
    public string SidebarStatus => !Launch.CanSelect||Launch.ShowError ? Launch.Status : Downloads.ActiveCount>0 ? "正在下载" : "已就绪";
    public string SidebarMessage => !Launch.CanSelect||Launch.ShowError ? Launch.Message : Downloads.ActiveCount>0 ? Downloads.ActiveCount+" 个安装任务正在进行" : "选择实例";
    private void RefreshSidebar(){Raise(nameof(SidebarStatus));Raise(nameof(SidebarMessage));}


    public ShellViewModel(LaunchViewModel launch, SHLauncher.Contracts.Engine.IEngineConfiguration? configuration = null, BackgroundSession? backgrounds = null)
    {
        ModsCommand=new(()=>Page=ShellPage.Mods);
        Backgrounds=backgrounds??new();
        Startup=new((configuration as SHLauncher.Application.Accounts.IAccountWorkspace)?.Accounts);
        Import = new(configuration as SHLauncher.Application.Install.IInstallationWorkspace,()=>Page=ShellPage.Downloads);
        Downloads = new(configuration as SHLauncher.Application.Install.IInstallationWorkspace);
        DownloadsCommand = new(() => Page = ShellPage.Downloads);
        Launch = launch; Settings = new(configuration, launch);
        launch.Refreshed += () => Raise(nameof(AccountMode)); Library = new(launch,Import.ChooseCommand,new DelegateCommand(()=>{Downloads.IsInstallerExpanded=true;Page=ShellPage.Downloads;}),()=>Page=ShellPage.Home); Home = new(launch, Library,()=>configuration?.IsMock==false?Startup.Accounts.ActiveProvider+" · 游戏数据保存在各自的实例中。":"演示资料库 · 此处的启动与运行状态为模拟。");
        Home.Accounts=Startup.Accounts;Home.AccountCommand=Startup.ManageAccountsCommand;
        HomeCommand = new(() => Page = ShellPage.Home);
        LibraryCommand = new(() => Page = ShellPage.Library);
        SettingsCommand = new(() => Page = ShellPage.Settings);
        Downloads.ImportCommand=Import.ChooseCommand;
        Launch.Refreshed += RefreshSidebar;
        Downloads.PropertyChanged += OnDownloadsChanged;
    }
    private void OnDownloadsChanged(object? sender,System.ComponentModel.PropertyChangedEventArgs e){if(e.PropertyName==nameof(Downloads.ActiveCount))RefreshSidebar();}
    public async ValueTask DisposeAsync() { Backgrounds.Dispose(); Launch.Refreshed-=RefreshSidebar;Downloads.PropertyChanged-=OnDownloadsChanged; Home.Dispose(); Library.Dispose(); await Startup.Accounts.DisposeAsync(); await Settings.DisposeAsync(); await Import.DisposeAsync(); await Launch.DisposeAsync(); await Downloads.DisposeAsync(); }
}
