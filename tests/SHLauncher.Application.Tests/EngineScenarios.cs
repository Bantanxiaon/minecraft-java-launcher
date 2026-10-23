using SHLauncher.Application.Launching;
using SHLauncher.Contracts.Engine;
using SHLauncher.Core.Launching;
using SHLauncher.Infrastructure.Mocks;

internal static class EngineScenarios
{
    private static void Check(bool value, string message)
    { if (!value) throw new InvalidOperationException(message); }

    private static MockLauncherEngine Engine(MockScenario scenario = MockScenario.Success) => new(new()
    { Scenario = scenario, StepDelay = TimeSpan.FromMilliseconds(5), RunningDuration = TimeSpan.FromMilliseconds(25) });

    private static async Task WaitState(LaunchCoordinator session, LauncherTaskState state)
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(3));
        while (session.Snapshot.State != state) await Task.Delay(1, timeout.Token);
    }

    internal static async Task RunAll()
    {
        var engine = Engine();
        var log = new MemoryLauncherLog();
        await using (var session = new LaunchCoordinator(engine, log))
        {
            await session.InitializeAsync();
            Check(session.Instances.Count == 3 && session.Selected?.Id == "elements", "Mock catalog");
            Check(!session.SelectInstance("absent"), "Invalid selection rejected");
            Check(session.SelectInstance("vanilla") && session.Selected?.JavaMajor == 21, "Selection updates metadata");
            var operation = session.StartAsync();
            var duplicates = await Task.WhenAll(Enumerable.Range(0, 30).Select(_ => session.StartAsync()));
            Check(duplicates.All(x => !x), "Rapid starts must not multiply work");
            Check(!session.SelectInstance("create"), "Selection locked during launch");
            await WaitState(session, LauncherTaskState.Running);
            Check(engine.StartedOperations == 1, "Exactly one engine operation after scheduling");
            Check(session.Snapshot.Progress == 1, "Running reports complete preparation");
            Check(await operation && session.Snapshot.State == LauncherTaskState.Idle && engine.ActiveOperations == 0, "Natural game exit");
            foreach (var target in new[] { LauncherTaskState.Preparing, LauncherTaskState.Launching, LauncherTaskState.Running })
            {
                operation = session.StartAsync();
                await WaitState(session, target);
                session.CancelOrStop();
                Check(session.Snapshot.State == (target == LauncherTaskState.Running ? LauncherTaskState.Stopping : LauncherTaskState.Cancelling), "Cancel has explicit transient state");
                await operation;
                Check(session.Snapshot.State == LauncherTaskState.Idle && engine.ActiveOperations == 0, "Cancel awaits engine cleanup");
            }
            for (var i = 0; i < 30; i++)
            {
                operation = session.StartAsync(); session.CancelOrStop(); await operation;
                Check(engine.ActiveOperations == 0 && session.Snapshot.State == LauncherTaskState.Idle, "Repeated cancellation leaked work");
            }
            Check(session.SelectInstance("create"), "Selection unlocked after cleanup");
        }
        Check(log.Entries.Count <= 128, "Log is bounded");
        foreach (var scenario in new[] { MockScenario.LaunchFailure, MockScenario.UnexpectedException, MockScenario.RuntimeMissing, MockScenario.InstanceInvalid })
        {
            engine = Engine(scenario);
            await using var session = new LaunchCoordinator(engine, new MemoryLauncherLog());
            await session.InitializeAsync(); await session.StartAsync();
            var expected = scenario switch { MockScenario.RuntimeMissing => LauncherErrorCode.RuntimeMissing, MockScenario.InstanceInvalid => LauncherErrorCode.InstanceInvalid, MockScenario.UnexpectedException => LauncherErrorCode.Unknown, _ => LauncherErrorCode.LaunchFailed };
            Check(session.Snapshot.State == LauncherTaskState.Failed && session.Snapshot.Error?.Code == expected && engine.ActiveOperations == 0, $"Failure mapping: {scenario}");
            if (scenario == MockScenario.LaunchFailure)
            {
                Check(await session.StartAsync() && session.Snapshot.State == LauncherTaskState.Idle, "Failure -> retry -> success");
            }
            else { session.ResetFailure(); Check(session.Snapshot.State == LauncherTaskState.Idle, "Reset failure"); }
        }
        engine = Engine(MockScenario.Cancellation);
        await using (var session = new LaunchCoordinator(engine, new MemoryLauncherLog()))
        {
            await session.InitializeAsync(); await session.StartAsync();
            Check(session.Snapshot.State == LauncherTaskState.Idle && engine.ActiveOperations == 0 && session.Snapshot.Error?.Code == LauncherErrorCode.Cancelled, "Engine-side cancellation");
        }
        engine = Engine(MockScenario.SlowLaunch);
        var closing = new LaunchCoordinator(engine, new MemoryLauncherLog());
        await closing.InitializeAsync(); var closingTask = closing.StartAsync();
        await closing.DisposeAsync(); await closingTask;
        Check(engine.ActiveOperations == 0 && !await closing.StartAsync(), "Window close terminates task and refuses restarts");
        await using (var session = new LaunchCoordinator(new InvalidEngine(), new MemoryLauncherLog()))
        {
            await session.InitializeAsync(); await session.StartAsync();
            Check(session.Snapshot.Error?.Code == LauncherErrorCode.Unknown, "Invalid engine transition is contained");
        }
        Console.WriteLine("PASS: success, failures, cancellation, stop, retry, selection, duplicates, close, invalid adapter");
    }

    private sealed class InvalidEngine : ILauncherEngine
    {
        public Task<IReadOnlyList<InstanceInfo>> GetInstancesAsync(CancellationToken token) => Engine().GetInstancesAsync(token);
        public async IAsyncEnumerable<EngineUpdate> LaunchAsync(LaunchRequest request,
            [System.Runtime.CompilerServices.EnumeratorCancellation] CancellationToken token)
        {
            await Task.Yield(); token.ThrowIfCancellationRequested();
            yield return new(LauncherTaskState.Running, .5, "Invalid transition");
        }
    }
}
