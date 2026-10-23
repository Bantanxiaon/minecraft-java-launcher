using SHLauncher.Contracts.Engine;
using SHLauncher.Core.Launching;

namespace SHLauncher.Application.Launching;

public sealed record LaunchSnapshot(Guid OperationId, long Sequence, string? InstanceId,
    LauncherTaskState State, double Progress, string Message, LauncherError? Error)
{
    public bool IsIndeterminate { get; init; }
    public static LaunchSnapshot Initial { get; } = new(Guid.Empty, 0, null, LauncherTaskState.Idle, 0, "一切就绪，开始新的旅程。", null);
    public bool CanLaunch => State is LauncherTaskState.Idle || State == LauncherTaskState.Failed && Error?.Retryable == true;
    public bool CanCancel => State is LauncherTaskState.Preparing or LauncherTaskState.Launching;
    public bool CanSelect => State is LauncherTaskState.Idle or LauncherTaskState.Failed;
}
