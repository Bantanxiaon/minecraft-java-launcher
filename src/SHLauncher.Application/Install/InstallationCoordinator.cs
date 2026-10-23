using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Engine;
using SHLauncher.Contracts.Install;
namespace SHLauncher.Application.Install;
public sealed class InstallationCoordinator(IInstallationBackend backend, IInstallationTaskStore store, IGameLogService logs) : IDownloadQueue
{
    private readonly object _gate = new();
    private readonly SemaphoreSlim _serial = new(1);
    private readonly Dictionary<Guid, Operation> _operations = new();
    private bool _closing, _initialized;
    private Task? _initialization;
    private readonly CancellationTokenSource _recoveryStop=new();
    public event Action? Changed;
    public string? PersistenceProblem { get; private set; }
    public IReadOnlyList<InstallationTaskRecord> Records { get { lock (_gate) return _operations.Values.Select(v => v.Record).Reverse().ToArray(); } }
    public IReadOnlyList<DownloadTaskSnapshot> Snapshot() => Records.Select(v => v.Snapshot).ToArray();
    public bool IsBusy => Snapshot().Any(s => Active(s.State));
    public Task InitializeAsync(CancellationToken token)
    {
        lock (_gate) return _initialization ??= Task.Run(() => InitializeCoreAsync(token), token);
    }
    private async Task InitializeCoreAsync(CancellationToken callerToken)
    {
        using var recovery=CancellationTokenSource.CreateLinkedTokenSource(callerToken,_recoveryStop.Token);
        var token=recovery.Token;
        try
        {
            var records = await store.LoadAsync(token).ConfigureAwait(false);
            foreach (var original in records.Reverse())
            {
                var record = Active(original.Snapshot.State) ? original with { Snapshot = original.Snapshot with {
                    State = DownloadTaskState.Cancelled, Problem = "上次任务已中断。重试将校验已有文件，未完成部分安全重下。", Stage = "已中断" } } : original;
                lock (_gate) _operations.TryAdd(record.Recipe.Id, new(record));
                if (record != original) await store.SaveAsync(record, token).ConfigureAwait(false);
            }
        }
        catch (InstallationPersistenceException e) { PersistenceProblem = e.Message; }
        lock (_gate) _initialized = true;
        Notify();
    }
    public bool Enqueue(InstallationRecipe recipe)
    {
        if (recipe.Id == Guid.Empty || string.IsNullOrWhiteSpace(recipe.DisplayName) || recipe.DisplayName.Length > 64 ||
            recipe.MinecraftVersion.Length is 0 or > 128 || recipe.LoaderVersion.Length > 128 || !Enum.IsDefined(recipe.Loader)) return false;
        if (recipe.InstanceKey == Guid.Empty) recipe = recipe with { InstanceKey = recipe.Id };
        Operation operation;
        lock (_gate)
        {
            if (!_initialized || _closing || PersistenceProblem is not null || _operations.ContainsKey(recipe.Id) || _operations.Values.Count(o => Active(o.Record.Snapshot.State)) >= 16 ||
                _operations.Values.Any(o => Active(o.Record.Snapshot.State) && (o.Record.Recipe.InstanceKey == recipe.InstanceKey || recipe.ArchiveSha256 is not null && string.Equals(o.Record.Recipe.ArchiveSha256,recipe.ArchiveSha256,StringComparison.OrdinalIgnoreCase)))) return false;
            foreach (var old in _operations.Where(p => !Active(p.Value.Record.Snapshot.State)).Take(Math.Max(0, _operations.Count - 63)).Select(p => p.Key).ToArray()) _operations.Remove(old);
            var title = recipe.DisplayName + " · Minecraft " + recipe.MinecraftVersion + (recipe.Loader == LoaderType.Vanilla ? "" : " · " + recipe.Loader + " " + recipe.LoaderVersion);
            operation = new(new(recipe, new(recipe.Id, title, DownloadTaskState.Queued, new(0, null), null, Stage: "等待安装")));
            _operations.Add(recipe.Id, operation);
            // Claim ownership before scheduling; a rapid duplicate cannot create a second worker.
            operation.Work = Task.Run(() => RunAsync(operation));
        }
        Notify(); return true;
    }
    public bool Retry(Guid id)
    {
        InstallationRecipe recipe;
        lock (_gate)
        {
            if (!_operations.TryGetValue(id, out var old) || old.Record.Snapshot.State is not (DownloadTaskState.Failed or DownloadTaskState.Cancelled)) return false;
            recipe = old.Record.Recipe with { Id = Guid.NewGuid() };
        }
        return Enqueue(recipe);
    }
    public async Task CancelAsync(Guid id)
    {
        Task work;
        lock (_gate)
        {
            if (!_operations.TryGetValue(id, out var operation)) return;
            if (Active(operation.Record.Snapshot.State)) operation.Stop.Cancel();
            work = operation.Work;
        }
        await work.ConfigureAwait(false);
    }
    public async Task ShutdownAsync()
    {
        Task? recovery;
        lock(_gate){_closing=true;_recoveryStop.Cancel();recovery=_initialization;}
        if(recovery is not null)
            try{await recovery.ConfigureAwait(false);}catch(OperationCanceledException){ }
        Task[] pending;
        lock (_gate)
        {
            _closing = true;
            foreach (var operation in _operations.Values.Where(o => Active(o.Record.Snapshot.State))) operation.Stop.Cancel();
            pending = _operations.Values.Select(o => o.Work).ToArray();
        }
        await Task.WhenAll(pending).ConfigureAwait(false);
    }
    private async Task RunAsync(Operation operation)
    {
        var acquired = false;
        try
        {
            await store.SaveAsync(operation.Record, operation.Stop.Token).ConfigureAwait(false);
            await _serial.WaitAsync(operation.Stop.Token).ConfigureAwait(false); acquired = true;
            Progress(operation, new("解析安装计划"));
            var result = await backend.InstallAsync(operation.Record.Recipe, new Forward(value => Progress(operation, value)), operation.Stop.Token).ConfigureAwait(false);
            Terminal(operation, DownloadTaskState.Completed, "安装完成", null, result);
        }
        catch (OperationCanceledException) when (operation.Stop.IsCancellationRequested)
        { Terminal(operation, DownloadTaskState.Cancelled, "已取消", "所有工作已停止。已验证文件保留，重试会重新校验。", null); }
        catch (InstallationPreconditionException e)
        {
            logs.Write(operation.Record.Recipe.Id,GameLogLevel.Warning,"install",e.Code+": "+e.Message);
            Terminal(operation,DownloadTaskState.Failed,"安装条件未满足",e.Message,null);
        }
        catch (Exception e)
        {
            logs.Write(operation.Record.Recipe.Id, GameLogLevel.Error, "install", e.GetType().Name + ": " + e.Message);
            Terminal(operation, DownloadTaskState.Failed, "安装未完成", "任务未完成，可查看详情后重试。", null);
        }
        finally
        {
            if (acquired) _serial.Release();
            try { await store.SaveAsync(operation.Record, CancellationToken.None).ConfigureAwait(false); }
            catch (InstallationPersistenceException e) { PersistenceProblem = e.Message; logs.Write(operation.Record.Recipe.Id, GameLogLevel.Error, "task-store", e.Message); }
            Notify();
        }
    }
    private void Progress(Operation operation, InstallProgress value)
    {
        lock (_gate)
        {
            if (!Active(operation.Record.Snapshot.State)) return;
            var before = operation.Record.Snapshot;
            var current = value.Download;
            var state = current?.State is DownloadTaskState.Downloading or DownloadTaskState.Verifying or DownloadTaskState.Finalizing ? current.State : DownloadTaskState.Resolving;
            operation.Record = operation.Record with { Snapshot = (current ?? before) with {
                Id = operation.Record.Recipe.Id, Title = before.Title, State = state, Stage = value.Stage, Problem = null } };
        }
        Notify();
    }
    private void Terminal(Operation operation, DownloadTaskState state, string stage, string? problem, InstallResult? result)
    {
        lock (_gate) operation.Record = operation.Record with { Snapshot = operation.Record.Snapshot with { State = state, Stage = stage, Problem = problem }, Result = result };
    }
    private void Notify()
    {
        foreach (var handler in Changed?.GetInvocationList() ?? [])
            try { ((Action)handler)(); }
            catch (Exception e) { logs.Write(Guid.Empty, GameLogLevel.Warning, "task-observer", e.GetType().Name); }
    }
    private static bool Active(DownloadTaskState state) => state is not (DownloadTaskState.Completed or DownloadTaskState.Cancelled or DownloadTaskState.Failed);
    private sealed class Operation(InstallationTaskRecord record)
    {
        internal InstallationTaskRecord Record = record;
        internal CancellationTokenSource Stop { get; } = new();
        internal Task Work = Task.CompletedTask;
    }
    private sealed class Forward(Action<InstallProgress> report) : IProgress<InstallProgress> { public void Report(InstallProgress value) => report(value); }
}
