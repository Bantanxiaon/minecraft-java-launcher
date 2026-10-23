using SHLauncher.Application.Install;
using SHLauncher.Contracts.Install;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Infrastructure.Logging;
internal static class InstallationScenarios
{
    internal static async Task Run()
    {
        var closingStore=new Store{DelayLoad=true};
        var closingQueue=new InstallationCoordinator(new Backend(),closingStore,new GameLogService());
        var recovery=closingQueue.InitializeAsync(default);await closingQueue.ShutdownAsync().WaitAsync(TimeSpan.FromSeconds(5));
        Check(recovery.IsCompleted&&!closingQueue.Enqueue(Recipe("after-recovery-close")),"close cancels and awaits recovery before any new work");
        var store = new Store { DelayLoad = true }; var backend = new Backend(); var logs = new GameLogService();
        var queue = new InstallationCoordinator(backend, store, logs);
        var initialized = queue.InitializeAsync(default);
        Check(!queue.Enqueue(Recipe("early")), "cannot enqueue before recovery finishes");
        store.Loaded.SetResult(); await initialized;
        var first = Recipe("one"); var second = Recipe("two");
        Check(queue.Enqueue(first), "first task accepted");
        await backend.Started.Task.WaitAsync(TimeSpan.FromSeconds(5));
        Check(!queue.Enqueue(first with { Id = Guid.NewGuid(), InstanceKey = first.Id }), "same instance cannot have duplicate installation ownership");
        Check(queue.Enqueue(second), "different instance queues behind current task");
        await queue.CancelAsync(second.Id);
        Check(backend.Starts == 1 && queue.Records.Single(r => r.Recipe.Id == second.Id).Snapshot.State == DownloadTaskState.Cancelled, "queued cancellation does not start backend");
        var pack=Recipe("pack") with { ArchiveSha256=new string('a',64) };
        Check(queue.Enqueue(pack),"first pack queues");
        Check(!queue.Enqueue(pack with{Id=Guid.NewGuid(),InstanceKey=Guid.NewGuid()}),"same archive cannot create duplicate active imports");
        await queue.ShutdownAsync();
        Check(backend.Active == 0 && !queue.IsBusy && queue.Records.All(r => r.Snapshot.State == DownloadTaskState.Cancelled), "close awaits all workers and terminal records");
        Check(!queue.Enqueue(Recipe("closed")), "closed owner cannot accept new work");
        var interrupted = Recipe("interrupted");
        store.Items[interrupted.Id] = new(interrupted, new(interrupted.Id, "Interrupted", DownloadTaskState.Downloading, new(10, 100), null));
        var retryBackend = new Backend { Complete = true }; var reopened = new InstallationCoordinator(retryBackend, store, logs);
        await reopened.InitializeAsync(default);
        Check(reopened.Records.Single(r => r.Recipe.Id == interrupted.Id).Snapshot.State == DownloadTaskState.Cancelled && retryBackend.Starts == 0, "restart marks unfinished task interrupted without automatic network");
        reopened.Changed += () => throw new InvalidOperationException("broken observer");
        Check(reopened.Retry(interrupted.Id), "interrupted task can explicitly retry");
        await Until(() => reopened.Records.Any(r => r.Snapshot.State == DownloadTaskState.Completed));
        Check(retryBackend.Starts == 1 && logs.Snapshot().Any(l => l.Source == "task-observer"), "observer failure cannot change successful installation");
        await reopened.ShutdownAsync();
        Check(store.Items.Values.Any(r => r.Snapshot.State == DownloadTaskState.Completed), "successful terminal state persisted");
        var phaseStore = new Store(); var phaseBackend = new Backend { ReportChildCompleted = true };
        var phaseQueue = new InstallationCoordinator(phaseBackend, phaseStore, logs); await phaseQueue.InitializeAsync(default);
        Check(phaseQueue.Enqueue(Recipe("phase")), "phase task accepted"); await phaseBackend.Started.Task;
        Check(phaseQueue.IsBusy && !phaseQueue.Records.Any(r => r.Snapshot.State == DownloadTaskState.Completed), "child batch completion cannot complete whole install");
        await phaseQueue.ShutdownAsync();
        Console.WriteLine("PASS: installation recovery, duplicate ownership, queued cancel, shutdown drain, explicit retry, observer isolation, child stage ownership.");
    }
    private static InstallationRecipe Recipe(string directory) => new(Guid.NewGuid(), "26.3", LoaderType.Vanilla, "", directory);
    private static void Check(bool value, string message) { if (!value) throw new Exception(message); }
    private static async Task Until(Func<bool> predicate)
    { for (var i = 0; i < 500; i++) { if (predicate()) return; await Task.Delay(10); } throw new Exception("Install state deadline exceeded."); }
    private sealed class Store : IInstallationTaskStore
    {
        internal readonly Dictionary<Guid,InstallationTaskRecord> Items = new();
        internal bool DelayLoad;
        internal readonly TaskCompletionSource Loaded = new(TaskCreationOptions.RunContinuationsAsynchronously);
        public async Task<IReadOnlyList<InstallationTaskRecord>> LoadAsync(CancellationToken token)
        { if (DelayLoad) await Loaded.Task.WaitAsync(token); lock (Items) return Items.Values.ToArray(); }
        public Task SaveAsync(InstallationTaskRecord record, CancellationToken token)
        { token.ThrowIfCancellationRequested(); lock (Items) Items[record.Recipe.Id] = record; return Task.CompletedTask; }
    }
    private sealed class Backend : IInstallationBackend
    {
        internal bool Complete, ReportChildCompleted;
        internal int Starts, Active;
        internal readonly TaskCompletionSource Started = new(TaskCreationOptions.RunContinuationsAsynchronously);
        public async Task<InstallResult> InstallAsync(InstallationRecipe recipe, IProgress<InstallProgress>? progress, CancellationToken token)
        {
            Interlocked.Increment(ref Starts); Interlocked.Increment(ref Active);
            try
            {
                if (ReportChildCompleted) progress?.Report(new("Base complete, loader still pending", new(recipe.Id, "Subtask", DownloadTaskState.Completed, new(1,1), null)));
                Started.TrySetResult();
                if (!Complete) await Task.Delay(Timeout.Infinite, token);
                return new(recipe.Id, "test", "test", 1, 0, 1);
            }
            finally { Interlocked.Decrement(ref Active); }
        }
    }
}
