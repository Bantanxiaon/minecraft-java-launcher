using SHLauncher.Core.Launching;

namespace SHLauncher.Contracts.Engine;

public sealed record LaunchRequest(string InstanceId, Guid SessionId = default);
public sealed record EngineUpdate(LauncherTaskState State, double Progress, string Message, bool IsIndeterminate = false);

public interface ILauncherEngine
{
    Task<IReadOnlyList<InstanceInfo>> GetInstancesAsync(CancellationToken cancellationToken);
    IAsyncEnumerable<EngineUpdate> LaunchAsync(LaunchRequest request, CancellationToken cancellationToken);
}
