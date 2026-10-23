using System.Runtime.CompilerServices;
using SHLauncher.Contracts.Engine;
using SHLauncher.Core.Launching;

namespace SHLauncher.Infrastructure.Mocks;

public sealed class MockLauncherEngine(MockLaunchOptions options) : ILauncherEngine
{
    private int _active;
    private int _started;
    public int ActiveOperations => Volatile.Read(ref _active);
    public int StartedOperations => Volatile.Read(ref _started);

    private static readonly IReadOnlyList<InstanceInfo> Catalog = Array.AsReadOnly(new[]
    {
        new InstanceInfo("elements", "元素觉醒", "1.20.1", "Forge", 17, 8192, "今天", "在山野与未知之间，找到自己的节奏。", "forest"),
        new InstanceInfo("vanilla", "Vanilla Survival", "1.21.1", "Vanilla", 21, 6144, "昨天", "从一方天地开始，慢慢建起属于你的世界。", "sand"),
        new InstanceInfo("create", "Create World", "1.20.1", "Fabric", 17, 8192, "3 天前", "让一个小小的想法，转动整个世界。", "slate")
    });

    public Task<IReadOnlyList<InstanceInfo>> GetInstancesAsync(CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        return Task.FromResult(Catalog);
    }

    public async IAsyncEnumerable<EngineUpdate> LaunchAsync(LaunchRequest request,
        [EnumeratorCancellation] CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        if (Interlocked.CompareExchange(ref _active, 1, 0) != 0) throw new InvalidOperationException("A mock operation is already active.");
        var attempt = Interlocked.Increment(ref _started);
        try
        {
            if (!Catalog.Any(x => x.Id == request.InstanceId)) throw Failure(LauncherErrorCode.InstanceInvalid);
            var delay = options.Scenario == MockScenario.SlowLaunch ? options.StepDelay * 4 : options.StepDelay;
            double[] steps = [0, .15, .35, .55, .75, .9, 1];
            string[] messages = ["准备实例 · 模拟", "检查运行环境 · 模拟", "准备游戏文件 · 模拟", "构建启动参数 · 模拟", "创建游戏进程 · 模拟", "等待游戏响应 · 模拟", "游戏运行中 · 模拟"];
            for (var i = 0; i < steps.Length; i++)
            {
                await Task.Delay(delay, cancellationToken).ConfigureAwait(false);
                if (i == 2 && options.Scenario == MockScenario.Cancellation) throw new OperationCanceledException(cancellationToken);
                if (i == 2 && options.Scenario == MockScenario.RuntimeMissing) throw Failure(LauncherErrorCode.RuntimeMissing);
                if (i == 2 && options.Scenario == MockScenario.InstanceInvalid) throw Failure(LauncherErrorCode.InstanceInvalid);
                if (i == 3 && options.Scenario == MockScenario.UnexpectedException) throw new InvalidOperationException("Simulated unexpected failure.");
                if (i == 4 && options.Scenario == MockScenario.LaunchFailure && attempt <= options.FailureAttempts) throw Failure(LauncherErrorCode.LaunchFailed);
                var state = i < 3 ? LauncherTaskState.Preparing : i < 6 ? LauncherTaskState.Launching : LauncherTaskState.Running;
                yield return new(state, steps[i], messages[i]);
            }
            await Task.Delay(options.RunningDuration, cancellationToken).ConfigureAwait(false);
            yield return new(LauncherTaskState.Idle, 1, "模拟游戏已退出。");
        }
        finally { Interlocked.Exchange(ref _active, 0); }
    }

    private static LauncherEngineException Failure(LauncherErrorCode code) => new(code switch
    {
        LauncherErrorCode.RuntimeMissing => new(code, "运行环境尚未就绪", "演示运行环境缺失。此提示没有进行真实 Java 检测。", "Mock runtime unavailable", false),
        LauncherErrorCode.InstanceInvalid => new(code, "这个实例暂时不可用", "演示实例信息无效。请返回资料库选择另一个实例。", "Mock instance invalid", false),
        _ => new(code, "这次没有成功启动", "模拟启动未能完成。实例没有被修改，你可以再试一次。", "Mock launch failure", true)
    });
}
