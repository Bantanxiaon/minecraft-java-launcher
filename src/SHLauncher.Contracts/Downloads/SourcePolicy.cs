namespace SHLauncher.Contracts.Downloads;
public enum DownloadSourceMode { Automatic, OfficialPreferred, MirrorPreferred }
public interface IDownloadSourcePreferences
{
    DownloadSourceMode Mode { get; }
    Task SetModeAsync(DownloadSourceMode mode, CancellationToken token);
}
public interface IDownloadSourceSelector
{
    DownloadSourceMode Mode { get; }
    IReadOnlyList<DownloadSource> Select(DownloadFile file);
}
