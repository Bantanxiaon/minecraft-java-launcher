using SHLauncher.Contracts.Engine;
using SHLauncher.Core.Launching;

namespace SHLauncher.Application.Launching;

public sealed class LaunchCoordinator(ILauncherEngine engine, ILauncherLog log) : IAsyncDisposable
{
    private ILauncherEngine _engine = engine;
    private readonly object _gate = new();
    private LaunchSnapshot _snapshot = LaunchSnapshot.Initial;
    private IReadOnlyList<InstanceInfo> _instances = Array.Empty<InstanceInfo>();
    private InstanceInfo? _selected;
    private CancellationTokenSource? _cancellation;
    private Task<bool>? _execution;
    private bool _disposed;

    public event Action? Changed;
    public LaunchSnapshot Snapshot { get { lock (_gate) return _snapshot; } }
    public InstanceInfo? Selected { get { lock (_gate) return _selected; } }
    public IReadOnlyList<InstanceInfo> Instances { get { lock (_gate) return _instances; } }

    public async Task InitializeAsync(CancellationToken token = default)
    {
        var instances = await _engine.GetInstancesAsync(token).ConfigureAwait(false);
        lock (_gate)
        {
            ObjectDisposedException.ThrowIf(_disposed, this);
            if (_instances.Count != 0) return;
            _instances = Array.AsReadOnly(instances.ToArray());
            _selected = _instances.FirstOrDefault();
        }
        Changed?.Invoke();
    }

    public async Task<bool> ChangeEngineAsync(ILauncherEngine replacement, CancellationToken token)
    {
        lock (_gate) if (_disposed || !_snapshot.CanSelect || _cancellation is not null) return false;
        var instances = await replacement.GetInstancesAsync(token).ConfigureAwait(false);
        lock (_gate)
        {
            if (_disposed || !_snapshot.CanSelect || _cancellation is not null) return false;
            _engine = replacement; _instances = Array.AsReadOnly(instances.ToArray());
            _selected = _instances.FirstOrDefault(x => x.IsLaunchable) ?? _instances.FirstOrDefault();
            Move(LauncherTaskState.Idle, 0, instances.Count == 0 ? "此目录未发现实例。" : "实例扫描完成。", null);
        }
        Changed?.Invoke(); return true;
    }
    public bool SelectInstance(string id)
    {
        lock (_gate)
        {
            if (_disposed || !_snapshot.CanSelect || _cancellation is not null) return false;
            var instance = _instances.FirstOrDefault(x => x.Id == id);
            if (instance is null) return false;
            _selected = instance;
            if (_snapshot.State == LauncherTaskState.Failed) Move(LauncherTaskState.Idle, 0, "已选择实例。", null);
        }
        Changed?.Invoke();
        return true;
    }

    public Task<bool> StartAsync()
    {
        Task<bool> execution;
        lock (_gate)
        {
            if (_disposed || !_snapshot.CanLaunch || _selected is null || _cancellation is not null)
                return Task.FromResult(false);
            _cancellation = new CancellationTokenSource();
            _snapshot = _snapshot with { OperationId = Guid.NewGuid(), InstanceId = _selected.Id };
            Move(LauncherTaskState.Preparing, 0, "准备实例", null);
            _execution = RunAsync(_snapshot.OperationId, new LaunchRequest(_selected.Id, _snapshot.OperationId), _cancellation.Token);
            execution = _execution;
        }
        Changed?.Invoke();
        return execution;
    }

    public void CancelOrStop()
    {
        lock (_gate)
        {
            if (_cancellation is null) return;
            if (_snapshot.CanCancel) Move(LauncherTaskState.Cancelling, _snapshot.Progress, "正在取消…", null);
            else if (_snapshot.State == LauncherTaskState.Running) Move(LauncherTaskState.Stopping, 1, "正在结束游戏…", null);
            else return;
            _cancellation.Cancel();
        }
        Changed?.Invoke();
    }

    public void ResetFailure()
    {
        lock (_gate)
        {
            if (_snapshot.State != LauncherTaskState.Failed || _cancellation is not null) return;
            Move(LauncherTaskState.Idle, 0, "已重置，可以重新出发。", null);
        }
        Changed?.Invoke();
    }

    private async Task<bool> RunAsync(Guid id, LaunchRequest request, CancellationToken token)
    {
        await Task.Yield();
        LauncherError? failure = null;
        string finishMessage = "游戏已退出。";
        try
        {
            await foreach (var update in _engine.LaunchAsync(request, token).WithCancellation(token).ConfigureAwait(false))
            {
                token.ThrowIfCancellationRequested();
                lock (_gate)
                {
                    if (id != _snapshot.OperationId) return false;
                    if (_snapshot.State is LauncherTaskState.Cancelling or LauncherTaskState.Stopping) continue;
                    if (update.State == LauncherTaskState.Idle)
                    {
                        if (_snapshot.State != LauncherTaskState.Running) throw new InvalidOperationException("Engine exited before running.");
                        break;
                    }
                    if (update.State is not (LauncherTaskState.Preparing or LauncherTaskState.Launching or LauncherTaskState.Running))
                        throw new InvalidOperationException("Unexpected engine state.");
                    if (!double.IsFinite(update.Progress) || update.Progress < _snapshot.Progress || update.Progress is < 0 or > 1)
                        throw new InvalidOperationException("Invalid engine progress.");
                    Move(update.State, update.Progress, update.Message, null);
                    _snapshot = _snapshot with { IsIndeterminate = update.IsIndeterminate };
                }
                Changed?.Invoke();
            }
            token.ThrowIfCancellationRequested();
            lock (_gate)
                if (_snapshot.State != LauncherTaskState.Running) throw new InvalidOperationException("Engine ended before running.");
        }
        catch (OperationCanceledException)
        {
            finishMessage = "任务已结束，后台操作已清理。";
            lock (_gate)
            {
                if (_snapshot.State is LauncherTaskState.Preparing or LauncherTaskState.Launching)
                    Move(LauncherTaskState.Cancelling, _snapshot.Progress, "正在取消…", null);
                else if (_snapshot.State == LauncherTaskState.Running)
                    Move(LauncherTaskState.Stopping, 1, "正在结束游戏…", null);
            }
        }
        catch (LauncherEngineException error) { failure = error.Error; }
        catch (Exception error)
        {
            failure = new(LauncherErrorCode.Unknown, "暂时无法开始", "启动流程遇到了意外问题。你可以重试。", error.GetType().Name, true);
        }
        // The iterator has already disposed before the slot is released and Idle is published.
        lock (_gate)
        {
            _cancellation?.Dispose();
            _cancellation = null;
            if (_snapshot.State is LauncherTaskState.Cancelling or LauncherTaskState.Stopping)
                Move(LauncherTaskState.Idle, 0, finishMessage, new(LauncherErrorCode.Cancelled, "任务已结束", finishMessage, null, false));
            else if (failure is not null) Move(LauncherTaskState.Failed, _snapshot.Progress, failure.Message, failure);
            else Move(LauncherTaskState.Idle, 0, finishMessage, null);
        }
        Changed?.Invoke();
        return true;
    }

    private void Move(LauncherTaskState state, double progress, string message, LauncherError? error)
    {
        if (state != _snapshot.State) LauncherTransitions.Ensure(_snapshot.State, state);
        _snapshot = _snapshot with { State = state, Progress = progress, Message = message, Error = error, IsIndeterminate = false, Sequence = _snapshot.Sequence + 1 };
        log.Write(new(_snapshot.OperationId, state, message, error?.Code));
    }

    public async ValueTask DisposeAsync()
    {
        Task<bool>? running;
        lock (_gate) { _disposed = true; running = _execution; }
        CancelOrStop();
        if (running is not null) await running.ConfigureAwait(false);
    }
}
