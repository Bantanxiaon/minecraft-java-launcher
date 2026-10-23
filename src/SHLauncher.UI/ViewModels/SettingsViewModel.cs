using SHLauncher.UI.Presentation;

namespace SHLauncher.UI.ViewModels;

public enum AppearanceTheme { System, Light, Dark }

public sealed class SettingsViewModel : ObservableModel, IAsyncDisposable
{
    private readonly SHLauncher.Contracts.Engine.IEngineConfiguration? _configuration;
    private readonly SHLauncher.Contracts.Downloads.IDownloadSourcePreferences? _sourcePreferences;
    private int _sourceMode;
    private bool _sourceReady;
    private readonly SemaphoreSlim _sourceWrite = new(1,1);
    private string _sourceStatus="全局设置，修改后自动保存";
    public string SourceStatus {get=>_sourceStatus;private set=>Set(ref _sourceStatus,value);}

    private string _renameName="";
    private string? _renameId;
    public string RenameName {get=>_renameName;set=>Set(ref _renameName,value);}
    public bool CanRename => Guid.TryParse(_renameId,out _);
    public AsyncCommand RenameCommand {get;}
    public string[] SourceModes { get; } = ["自动选择（推荐）","官方源优先","镜像源优先"];
    public int SourceMode { get => _sourceMode; set {
        if(value is >=0 and <=2 && Set(ref _sourceMode,value) && _sourceReady)SaveSourceCommand.Execute(null);
    } }
    public AsyncCommand SaveSourceCommand { get; }

    private SHLauncher.Contracts.Platform.IFileDialogService? _dialogs;
    private string _connectionMessage = "";
    private string _javaPath = "";
    public string ConnectionMessage { get => _connectionMessage; private set => Set(ref _connectionMessage, value); }
    public bool ConfirmRunningClose => _configuration?.IsMock == false;
    public string RecentGameLogs => _configuration is null ? "" : string.Join("\n", _configuration.GameLogs.Snapshot()
        .TakeLast(12).Select(x => $"{x.Timestamp:HH:mm:ss} [{x.Level}] [{x.SessionId.ToString("N")[..8]}] {x.Message}"));
    public DelegateCommand RefreshLogsCommand { get; } = new(() => { });
    public string RootLabel => _configuration is null || _configuration.IsMock ? "演示模式" : _configuration.Root;
    public string JavaLabel => string.IsNullOrWhiteSpace(JavaPath) ? "自动检测符合实例要求的 Java；也可手动选择。" : JavaPath;
    public string JavaPath { get => _javaPath; set { if (Set(ref _javaPath, value)) { Raise(nameof(JavaLabel)); if(_configuration is not null)_configuration.ManualJava = value; } } }
    public AsyncCommand ChooseRootCommand { get; }
    public AsyncCommand ChooseJavaCommand { get; }
    public AsyncCommand MockModeCommand { get; }
    public void AttachDialogs(SHLauncher.Contracts.Platform.IFileDialogService dialogs) => _dialogs = dialogs;
    public SettingsViewModel(SHLauncher.Contracts.Engine.IEngineConfiguration? configuration = null, LaunchViewModel? launch = null)
    {
        _configuration = configuration;
        RenameCommand=new(async()=>{
            if(configuration is SHLauncher.Application.Install.IInstallationWorkspace workspace && Guid.TryParse(_renameId,out var id))
                ConnectionMessage=await workspace.RenameInstanceAsync(id,RenameName,default);
        },()=>CanRename && launch?.CanSelect==true,e=>ConnectionMessage="名称未更新："+e.GetType().Name);
        _sourcePreferences = (configuration as SHLauncher.Application.Install.IInstallationWorkspace)?.SourcePreferences;
        _sourceMode = (int)(_sourcePreferences?.Mode ?? SHLauncher.Contracts.Downloads.DownloadSourceMode.Automatic);
        SaveSourceCommand = new(async () => {
            if (_sourcePreferences is null) return;
            var mode=SourceMode;SourceStatus="正在保存…";
            await _sourceWrite.WaitAsync();
            try {
                if(mode!=SourceMode)return;
                await _sourcePreferences.SetModeAsync((SHLauncher.Contracts.Downloads.DownloadSourceMode)mode,default);
                if(mode==SourceMode)SourceStatus="已自动保存";
            } finally {_sourceWrite.Release();}
        },()=>_sourcePreferences is not null,e=>SourceStatus="未能保存，请重新选择后重试："+e.GetType().Name);
        _sourceReady=true;
        RefreshLogsCommand = new(() => Raise(nameof(RecentGameLogs)));
        ChooseRootCommand = new(async () =>
        {
            if (_dialogs is null || _configuration is null) return;
            var folder = await _dialogs.PickFolderAsync(CancellationToken.None);
            if (folder is null) return;
            ConnectionMessage = await _configuration.UseRootAsync(folder, CancellationToken.None); Raise(nameof(RootLabel));
        }, () => launch?.CanSelect == true, e => ConnectionMessage = "目录读取未完成：" + e.GetType().Name);
        ChooseJavaCommand = new(async () =>
        {
            if (_dialogs is null || _configuration is null) return;
            var file = await _dialogs.PickJavaAsync(CancellationToken.None);
            if (file is null) return;
            JavaPath = file;
            ConnectionMessage = "已选择 Java。启动时将校验版本和架构，不会静默换用其他 Java。";
            await Task.CompletedTask;
        }, () => launch?.CanSelect == true, e => ConnectionMessage = "Java 选择未完成：" + e.GetType().Name);
        MockModeCommand = new(async () =>
        {
            if (_configuration is null) return;
            ConnectionMessage = await _configuration.UseMockAsync(CancellationToken.None); Raise(nameof(RootLabel));
        }, () => launch?.CanSelect == true, e => ConnectionMessage = "模式切换未完成：" + e.GetType().Name);
        if (launch is not null) launch.Refreshed += () => {
            if(_renameId!=launch.Selected?.Id){_renameId=launch.Selected?.Id;RenameName=launch.Selected?.Name??"";Raise(nameof(CanRename));}
            RenameCommand.Refresh(); Raise(nameof(RootLabel)); Raise(nameof(ConfirmRunningClose)); ChooseRootCommand.Refresh(); ChooseJavaCommand.Refresh(); MockModeCommand.Refresh(); };
    }
    public async ValueTask DisposeAsync()
    {
        await Task.WhenAll(ChooseRootCommand.DrainAsync(),ChooseJavaCommand.DrainAsync(),MockModeCommand.DrainAsync(),
            SaveSourceCommand.DrainAsync(),RenameCommand.DrainAsync());
        _sourceWrite.Dispose();
    }
    private int _theme;
    private bool _reduced;
    private double _memory = 8192;
    private string _runtime = "Java 17 · 演示";
    public string[] Themes { get; } = ["跟随系统", "浅色", "深色"];
    public string[] Runtimes { get; } = ["Java 17 · 演示", "Java 21 · 演示"];
    public int ThemeIndex { get => _theme; set { if (value is >= 0 and <= 2 && Set(ref _theme, value)) AppearanceChanged?.Invoke(); } }
    public AppearanceTheme Theme => (AppearanceTheme)_theme;
    public bool ReducedMotion { get => _reduced; set { if (Set(ref _reduced, value)) AppearanceChanged?.Invoke(); } }
    public double MemoryMb { get => _memory; set { if (Set(ref _memory, Math.Clamp(value, 2048, 16384))) Raise(nameof(MemoryLabel)); } }
    public string MemoryLabel => $"{MemoryMb / 1024:0} GB";
    public string SelectedRuntime { get => _runtime; set => Set(ref _runtime, value); }
    public event Action? AppearanceChanged;
}
