using SHLauncher.Core.Launching;

namespace SHLauncher.Contracts.Engine;

public sealed record LaunchLogEntry(Guid OperationId, LauncherTaskState State, string Event,
    LauncherErrorCode? ErrorCode = null);

public interface ILauncherLog { void Write(LaunchLogEntry entry); }
