using Avalonia.Threading;
using SHLauncher.Application.Launching;
using SHLauncher.Contracts.Engine;
using SHLauncher.Core.Launching;
using SHLauncher.UI.Presentation;

namespace SHLauncher.UI.ViewModels;

public sealed class LaunchViewModel : ObservableModel, IAsyncDisposable
{
    private readonly LaunchCoordinator _session;
    private bool _detached;
    private string? _interactionMessage;
    private readonly ILauncherLog _log;
    public LaunchSnapshot Snapshot => _session.Snapshot;
    public InstanceInfo? Selected => _session.Selected;
    public IReadOnlyList<InstanceInfo> Instances => _session.Instances;
    public AsyncCommand StartCommand { get; }
    public DelegateCommand CancelCommand { get; }
    public DelegateCommand ResetCommand { get; }
    public bool CanSelect => Snapshot.CanSelect;
    public bool ShowProgress => Snapshot.State is not (LauncherTaskState.Idle or LauncherTaskState.Failed);
    public bool ShowCancel => Snapshot.CanCancel;
    public bool ShowError => Snapshot.State == LauncherTaskState.Failed;
    public bool IsRunning => Snapshot.State == LauncherTaskState.Running;
    public bool MainEnabled => Selected?.IsLaunchable == true && (Snapshot.CanLaunch || IsRunning);
    public double Progress => Snapshot.Progress * 100;
    public bool IsIndeterminate => Snapshot.IsIndeterminate;
    public string ProgressText => IsIndeterminate ? "正在处理…" : $"{Progress:0}%";
    public string Status => Snapshot.State switch
    {
        LauncherTaskState.Idle => "已就绪", LauncherTaskState.Preparing => "准备中", LauncherTaskState.Launching => "启动中",
        LauncherTaskState.Running => "游戏运行中", LauncherTaskState.Cancelling => "正在取消", LauncherTaskState.Stopping => "正在结束", _ => "启动未完成"
    };
    public bool PresentationCanStart => MainEnabled && !IsRunning;
    public string PresentationLabel => Snapshot.State switch { LauncherTaskState.Idle => "▶ 启动游戏", LauncherTaskState.Preparing => "准备中", LauncherTaskState.Launching => "启动中", LauncherTaskState.Running => "游戏运行中", LauncherTaskState.Cancelling => "正在取消", LauncherTaskState.Stopping => "正在结束", _ => "启动失败" };
    public string MainLabel => Snapshot.State switch
    {
        LauncherTaskState.Idle => "▶ 开始游戏", LauncherTaskState.Failed => Snapshot.Error?.Retryable == true ? "重新尝试" : "请先检查实例", LauncherTaskState.Running => "结束游戏",
        LauncherTaskState.Launching => "正在启动…", LauncherTaskState.Cancelling => "正在取消…", LauncherTaskState.Stopping => "正在结束…", _ => "正在准备…"
    };
    public string Message => _interactionMessage ?? Snapshot.Message;
    public string ErrorTitle => Snapshot.Error?.Title ?? "";
    public string ErrorMessage => ShowError ? Snapshot.Error?.Message ?? "" : Selected?.Problem?.Message ?? "";
    public event Action? Refreshed;

    public LaunchViewModel(LaunchCoordinator session, ILauncherLog log)
    {
        _session = session; _log = log;
        StartCommand = new(async () =>
        {
            if (IsRunning) _session.CancelOrStop();
            else await _session.StartAsync();
        }, () => MainEnabled, error =>
        {
            _log.Write(new(Snapshot.OperationId, Snapshot.State, "UI command failed: " + error.GetType().Name, LauncherErrorCode.Unknown));
            _interactionMessage = "界面操作没有完成，请重新尝试。"; Raise(nameof(Message));
        });
        CancelCommand = new(_session.CancelOrStop, () => ShowCancel);
        ResetCommand = new(_session.ResetFailure, () => ShowError);
        _session.Changed += OnChanged;
    }

    public Task InitializeAsync() => _session.InitializeAsync();
    public bool Select(string id) => _session.SelectInstance(id);
    public Task<bool> StartAsync() => _session.StartAsync();
    public void Cancel() => _session.CancelOrStop();

    private void OnChanged()
    {
        if (Dispatcher.UIThread.CheckAccess()) Refresh();
        else Dispatcher.UIThread.Post(Refresh);
    }

    private void Refresh()
    {
        if (_detached) return;
        string[] names = [nameof(Snapshot), nameof(Selected), nameof(Instances), nameof(CanSelect), nameof(ShowProgress), nameof(ShowCancel), nameof(ShowError), nameof(IsRunning), nameof(MainEnabled), nameof(Progress), nameof(ProgressText), nameof(IsIndeterminate), nameof(Status), nameof(MainLabel), nameof(PresentationLabel), nameof(PresentationCanStart), nameof(Message), nameof(ErrorTitle), nameof(ErrorMessage)];
        foreach (var name in names) Raise(name);
        StartCommand.Refresh(); CancelCommand.Refresh(); ResetCommand.Refresh(); Refreshed?.Invoke();
    }

    public async ValueTask DisposeAsync()
    {
        _detached = true; _session.Changed -= OnChanged;
        await _session.DisposeAsync();
    }
}
