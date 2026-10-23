namespace SHLauncher.Contracts.Engine;

public enum LauncherErrorCode { RuntimeMissing, InstanceInvalid, LaunchFailed, Cancelled, Unknown, InstanceNotFound, MissingMetadata, UnsupportedVersion, JavaVersionMismatch, GameFileMissing, LaunchArgumentError, NativeExtractionError, ProcessStartFailed, GameExitedUnexpectedly }

public sealed record LauncherError(LauncherErrorCode Code, string Title, string Message,
    string? TechnicalDetails, bool Retryable);

public sealed class LauncherEngineException(LauncherError error) : Exception(error.Message)
{
    public LauncherError Error { get; } = error;
}
