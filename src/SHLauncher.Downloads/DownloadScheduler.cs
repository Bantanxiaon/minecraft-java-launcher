namespace SHLauncher.Downloads;
public sealed class DownloadScheduler(int files = 4, int connections = 8) : IDisposable
{
    internal SemaphoreSlim Files { get; } = new(files > 0 ? files : throw new ArgumentOutOfRangeException(nameof(files)));
    internal SemaphoreSlim Connections { get; } = new(connections > 0 ? connections : throw new ArgumentOutOfRangeException(nameof(connections)));
    private int _connections, _peakConnections;
    public int ActiveConnections => Volatile.Read(ref _connections);
    public int PeakConnections => Volatile.Read(ref _peakConnections);
    internal async Task AcquireConnectionAsync(CancellationToken token)
    {
        await Connections.WaitAsync(token).ConfigureAwait(false);
        var current = Interlocked.Increment(ref _connections); int previous;
        do { previous = _peakConnections; if (previous >= current) break; }
        while (Interlocked.CompareExchange(ref _peakConnections, current, previous) != previous);
    }
    internal void ReleaseConnection() { Interlocked.Decrement(ref _connections); Connections.Release(); }
    private readonly SemaphoreSlim[] _destinations = Enumerable.Range(0, 64).Select(_ => new SemaphoreSlim(1)).ToArray();
    private readonly SemaphoreSlim[] _cacheKeys = Enumerable.Range(0, 64).Select(_ => new SemaphoreSlim(1)).ToArray();
    internal SemaphoreSlim CacheKey(string key) => _cacheKeys[(StringComparer.Ordinal.GetHashCode(key) & int.MaxValue) % _cacheKeys.Length];
    internal SemaphoreSlim Destination(string path)
    {
        var comparer = OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal;
        return _destinations[(comparer.GetHashCode(path) & int.MaxValue) % _destinations.Length];
    }
    public void Dispose() { Files.Dispose(); Connections.Dispose(); foreach (var gate in _destinations.Concat(_cacheKeys)) gate.Dispose(); }
}
public sealed record DownloadOptions(int FileParallelism = 4, int SegmentsPerFile = 2, long SegmentThreshold = 8 * 1024 * 1024,
    int AttemptsPerSource = 3, TimeSpan? ReadTimeout = null, long MaxUnknownSize = 64 * 1024 * 1024)
{
    internal TimeSpan Timeout => ReadTimeout ?? TimeSpan.FromSeconds(30);
}
