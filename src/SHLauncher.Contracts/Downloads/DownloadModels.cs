namespace SHLauncher.Contracts.Downloads;
public enum DownloadTaskState { Queued, Resolving, Downloading, Verifying, Finalizing, Completed, Failed, Cancelling, Cancelled }
public sealed record DownloadSource(string Id, Uri Uri, bool IsOfficial = true);
public enum DownloadContentKind { Binary, JsonMetadata, Sha1Metadata, XmlMetadata }
public sealed record DownloadFile(string Destination, long? ExpectedSize, string? Sha1, string? Sha256, IReadOnlyList<DownloadSource> Sources, DownloadContentKind ContentKind = DownloadContentKind.Binary);
public sealed record DownloadRequest(Guid Id, string Title, string AllowedRoot, IReadOnlyList<DownloadFile> Files);
public sealed record DownloadProgress(long CompletedBytes, long? TotalBytes, long NetworkBytes = 0, double BytesPerSecond = 0);
public sealed record DownloadTaskSnapshot(Guid Id, string Title, DownloadTaskState State, DownloadProgress Progress,
    string? Problem, int CompletedFiles = 0, int TotalFiles = 0, string? Stage = null);
public sealed record DownloadFailure(string Destination, string? SourceId, string Code, string Message);
public sealed record DownloadFileResult(string Destination, string? SourceId, bool Reused, long NetworkBytes);
public sealed record DownloadResult(Guid Id, bool Succeeded, bool Cancelled, IReadOnlyList<DownloadFileResult> Files, IReadOnlyList<DownloadFailure> Failures);
public sealed record DownloadEvent(Guid SessionId, DateTimeOffset Timestamp, string? SourceId, string Kind, string Message);
public interface IDownloadManager
{
    Task<DownloadResult> DownloadAsync(DownloadRequest request, IProgress<DownloadTaskSnapshot>? progress, CancellationToken token);
    IReadOnlyList<DownloadEvent> Events(Guid? sessionId = null);
}
public interface IDownloadTask { DownloadTaskSnapshot Snapshot { get; } Task CancelAsync(CancellationToken token); }
public interface IDownloadQueue { IReadOnlyList<DownloadTaskSnapshot> Snapshot(); }


public interface IDownloadEventSink { void Write(DownloadEvent entry); }

