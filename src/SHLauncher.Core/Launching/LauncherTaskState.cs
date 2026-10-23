namespace SHLauncher.Core.Launching;

public enum LauncherTaskState { Idle, Preparing, Launching, Running, Stopping, Cancelling, Failed }

public static class LauncherTransitions
{
    public static bool CanMove(LauncherTaskState from, LauncherTaskState to) => (from, to) switch
    {
        (LauncherTaskState.Idle, LauncherTaskState.Preparing) => true,
        (LauncherTaskState.Preparing, LauncherTaskState.Launching or LauncherTaskState.Cancelling or LauncherTaskState.Failed) => true,
        (LauncherTaskState.Launching, LauncherTaskState.Running or LauncherTaskState.Cancelling or LauncherTaskState.Failed) => true,
        (LauncherTaskState.Running, LauncherTaskState.Idle or LauncherTaskState.Stopping or LauncherTaskState.Failed) => true,
        (LauncherTaskState.Cancelling or LauncherTaskState.Stopping, LauncherTaskState.Idle) => true,
        (LauncherTaskState.Failed, LauncherTaskState.Idle or LauncherTaskState.Preparing) => true,
        _ => false
    };

    public static void Ensure(LauncherTaskState from, LauncherTaskState to)
    {
        if (!CanMove(from, to)) throw new InvalidOperationException($"Illegal launcher transition: {from} -> {to}");
    }
}
