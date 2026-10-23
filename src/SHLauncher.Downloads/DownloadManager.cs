using System.Collections.Concurrent;
using System.Diagnostics;
using SHLauncher.Contracts.Downloads;
namespace SHLauncher.Downloads;
public sealed class DownloadManager : IDownloadManager, IDisposable
{
    private readonly DownloadScheduler _scheduler;
    private readonly DownloadOptions _options;
    private readonly HttpDownloadTransport _transport;
    private readonly string _cache;
    private readonly ConcurrentQueue<DownloadEvent> _events = new();
    private readonly Func<string, long> _freeSpace;
    private readonly IDownloadEventSink? _eventSink;
    private readonly IDownloadSourceSelector _sourceSelector;
    private int _sinkFailed;
    private readonly Func<string,string>? _redact;
    public DownloadManager(DownloadScheduler scheduler, string cache, DownloadOptions? options = null, Func<string, long>? freeSpace = null, IDownloadEventSink? eventSink = null, IDownloadSourceSelector? sourceSelector = null, Func<Uri,IReadOnlyDictionary<string,string>>? authorize = null,Func<string,string>? redact=null)
    {
        _scheduler = scheduler; _options = options ?? new(); _eventSink = eventSink; _sourceSelector = sourceSelector ?? new DownloadSourceSelector();_redact=redact;
        if (_options.FileParallelism <= 0 || _options.SegmentsPerFile <= 0 || _options.AttemptsPerSource is < 1 or > 5 || _options.Timeout <= TimeSpan.Zero) throw new ArgumentException("Invalid download options.");
        _cache = Path.GetFullPath(cache); Directory.CreateDirectory(_cache);
        _transport = new(scheduler, _options, authorize); _freeSpace = freeSpace ?? (path => new DriveInfo(Path.GetPathRoot(path)!).AvailableFreeSpace);
    }
    public IReadOnlyList<DownloadEvent> Events(Guid? sessionId = null) => _events.Where(x => sessionId is null || x.SessionId == sessionId).ToArray();
    public async Task<DownloadResult> DownloadAsync(DownloadRequest request, IProgress<DownloadTaskSnapshot>? progress, CancellationToken token)
    {
        var files = request.Files.Select(file => Validate(request.AllowedRoot, file)).ToArray();
        var comparer = OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal;
        if (files.GroupBy(f => f.Destination, comparer).Any(g => g.Count() > 1)) throw new ArgumentException("Duplicate destinations must be resolved before downloading.");
        var results = new ConcurrentBag<DownloadFileResult>(); var failures = new ConcurrentBag<DownloadFailure>();
        var watch = Stopwatch.StartNew(); long inbound = 0, completedBytes = 0, lastReport = 0; int complete = 0;
        var reportGate = new object(); var reportedStates = new HashSet<DownloadTaskState>();
        var total = files.All(f => f.ExpectedSize is not null) ? files.Sum(f => f.ExpectedSize!.Value) : (long?)null;
        void Report(DownloadTaskState state, string? stage = null)
        {
            lock (reportGate)
            {
                var first = reportedStates.Add(state);
                if (!first && state is not (DownloadTaskState.Completed or DownloadTaskState.Failed or DownloadTaskState.Cancelled) && watch.ElapsedMilliseconds - lastReport < 200) return;
                lastReport = watch.ElapsedMilliseconds;
            }
            var snapshot = new DownloadTaskSnapshot(request.Id, request.Title, state,
                new(Interlocked.Read(ref completedBytes), total, Interlocked.Read(ref inbound), inbound / Math.Max(watch.Elapsed.TotalSeconds, .001)),
                failures.FirstOrDefault()?.Message, Volatile.Read(ref complete), files.Length, stage);
            progress?.Report(snapshot);
        }
        void Received(int count)
        {
            Interlocked.Add(ref inbound, count);
            var now = watch.ElapsedMilliseconds;
            if (now - Interlocked.Read(ref lastReport) >= 200) Report(DownloadTaskState.Downloading);
        }
        Report(DownloadTaskState.Queued);
        try
        {
            await Parallel.ForEachAsync(files, new ParallelOptions { MaxDegreeOfParallelism = _options.FileParallelism, CancellationToken = token }, async (file, ct) =>
            {
                await _scheduler.Files.WaitAsync(ct).ConfigureAwait(false);
                var gate = _scheduler.Destination(file.Destination);
                try
                {
                    await gate.WaitAsync(ct).ConfigureAwait(false);
                    try
                    {
                        Report(DownloadTaskState.Resolving, "Checking existing files and verified cache");
                        var result = await FileAsync(request.Id, file, Received, state => Report(state), ct).ConfigureAwait(false);
                        results.Add(result); Interlocked.Increment(ref complete); Interlocked.Add(ref completedBytes, file.ExpectedSize ?? new FileInfo(file.Destination).Length);
                        Report(DownloadTaskState.Downloading, "File finalized");
                    }
                    catch (OperationCanceledException) when (ct.IsCancellationRequested) { throw; }
                    catch (Exception e) when (e is IOException or InvalidDataException or HttpRequestException or UnauthorizedAccessException or OperationCanceledException)
                    {
                        var code = e is DownloadFailedException failure ? failure.Code : e.GetType().Name;
                        failures.Add(new(file.Destination, (e as DownloadFailedException)?.SourceId, code, _redact?.Invoke(e.Message)??e.Message));
                        Trace(request.Id, (e as DownloadFailedException)?.SourceId, "file-failure", "target=" + file.Destination + "; " + code + "; " + e.Message);
                    }
                    finally { gate.Release(); }
                }
                finally { _scheduler.Files.Release(); }
            }).ConfigureAwait(false);
        }
        catch (OperationCanceledException) when (token.IsCancellationRequested)
        {
            Report(DownloadTaskState.Cancelled, "All workers stopped; partial files retained for diagnosis, next attempt starts fresh");
            return new(request.Id, false, true, results.ToArray(), failures.ToArray());
        }
        Report(failures.IsEmpty ? DownloadTaskState.Completed : DownloadTaskState.Failed);
        return new(request.Id, failures.IsEmpty, false, results.ToArray(), failures.ToArray());
    }
    private async Task<DownloadFileResult> FileAsync(Guid id, DownloadFile file, Action<int> received, Action<DownloadTaskState> stage, CancellationToken token)
    {
        var key = FileIntegrity.CacheKey(file); var cache = key is null ? null : Path.Combine(_cache, key);
        if (key is not null && await FileIntegrity.VerifyAsync(file.Destination, file, token).ConfigureAwait(false))
        { Trace(id, null, "reuse", "Destination hash verified; target=" + file.Destination + "; key=" + key); return new(file.Destination, "existing", true, 0); }
        Directory.CreateDirectory(Path.GetDirectoryName(file.Destination)!);
        if (cache is not null)
        {
            var cacheGate = _scheduler.CacheKey(key!); await cacheGate.WaitAsync(token).ConfigureAwait(false);
            try
            {
                if (await FileIntegrity.VerifyAsync(cache, file, token).ConfigureAwait(false))
                {
                    var part = Part(file.Destination); File.Copy(cache, part, false);
                    if (!await FileIntegrity.VerifyAsync(part, file, token).ConfigureAwait(false)) throw new DownloadFailedException("CacheHashMismatch", "Cache copy integrity failed.");
                    token.ThrowIfCancellationRequested(); stage(DownloadTaskState.Finalizing); File.Move(part, file.Destination, true);
                    Trace(id, null, "reuse", "Content-addressed cache verified and copied; target=" + file.Destination + "; key=" + key); return new(file.Destination, "cache", true, 0);
                }
            }
            finally { cacheGate.Release(); }
        }
        if (_freeSpace(file.Destination) < (file.ExpectedSize ?? _options.MaxUnknownSize) + 8 * 1024 * 1024) throw new DownloadFailedException("DiskSpace", "Insufficient space for verified download.");
        Exception? last = null; string? lastSource = null; long bytes = 0;
        var sources = _sourceSelector.Select(file);
        foreach (var source in sources) HttpDownloadTransport.ValidateSource(source.Uri);
        Trace(id, null, "source-policy", "target=" + file.Destination + "; mode=" + _sourceSelector.Mode + "; candidates=" +
            string.Join(" | ", sources.Select(s => s.Id + ":" + s.Uri.GetLeftPart(UriPartial.Path))));
        foreach (var source in sources)
        {
            lastSource = source.Id; var ranges = true;
            for (var attempt = 0; attempt < _options.AttemptsPerSource; attempt++)
            {
                token.ThrowIfCancellationRequested(); var part = Part(file.Destination);
                Trace(id, source.Id, "source", $"Attempt {attempt + 1}; {source.Uri.GetLeftPart(UriPartial.Path)}");
                try
                {
                    stage(DownloadTaskState.Downloading);
                    await _transport.DownloadAsync(file, source, part, count => { Interlocked.Add(ref bytes, count); received(count); },
                        message => Trace(id, source.Id, "range", message), ranges, token).ConfigureAwait(false);
                    stage(DownloadTaskState.Verifying);
                    if (!await FileIntegrity.VerifyAsync(part, file, token, message => Trace(id, source.Id, "integrity", "target=" + file.Destination + "; " + message)).ConfigureAwait(false)) throw new DownloadFailedException("HashMismatch", "Downloaded size or hash does not match metadata.");
                    token.ThrowIfCancellationRequested(); stage(DownloadTaskState.Finalizing);
                    File.Move(part, file.Destination, true);
                    Trace(id, source.Id, "complete", $"Verified file atomically finalized; target={file.Destination}; payload bytes={bytes}.");
                    if (cache is not null)
                    {
                        var cacheGate = _scheduler.CacheKey(key!); await cacheGate.WaitAsync(CancellationToken.None).ConfigureAwait(false);
                        try
                        {
                            if (!await FileIntegrity.VerifyAsync(cache, file, CancellationToken.None).ConfigureAwait(false))
                            { var cachedPart = Part(cache); File.Copy(file.Destination, cachedPart, false); File.Move(cachedPart, cache, true); }
                        }
                        catch (Exception cacheError) when (cacheError is IOException or UnauthorizedAccessException)
                        { Trace(id, source.Id, "cache", "Cache write unavailable; destination is already verified."); }
                        finally { cacheGate.Release(); }
                    }
                    return new(file.Destination, source.Id, false, bytes);
                }
                catch (OperationCanceledException) when (token.IsCancellationRequested) { throw; }
                catch (Exception e) when (e is IOException or InvalidDataException or HttpRequestException or OperationCanceledException)
                {
                    last = e; Trace(id, source.Id, "failure", e.GetType().Name + ": " + e.Message);
                    if (e is RangeProtocolException && ranges) { ranges = false; attempt--; Trace(id, source.Id, "range", "Range failed; retrying this source with a fresh single-connection part."); continue; }
                    if (e is DownloadFailedException or InvalidDataException) break;
                    if (e is SourceHttpException http && http.Status is not (408 or 429 or 500 or 502 or 503 or 504)) break;
                    if (attempt + 1 >= _options.AttemptsPerSource) break;
                    var wait = e is SourceHttpException status && status.RetryAfter is { } delay && delay > TimeSpan.Zero ? delay : TimeSpan.FromMilliseconds(300 * (1 << attempt));
                    if (wait > TimeSpan.FromMinutes(2)) { Trace(id, source.Id, "disabled", "Retry-After exceeds interactive budget; not retrying this source early."); break; }
                    Trace(id, source.Id, "retry", "Waiting " + wait.TotalMilliseconds + " ms"); await Task.Delay(wait, token).ConfigureAwait(false);
                }
            }
            Trace(id, source.Id, "disabled", "Source exhausted; advancing to the next configured source.");
        }
        var failureCode = last switch { DownloadFailedException final => final.Code, SourceHttpException http => "HTTP" + http.Status, _ => "SourcesExhausted" };
        throw new DownloadFailedException(failureCode, "All configured sources failed; " + last?.Message, lastSource);
    }
    private static DownloadFile Validate(string root, DownloadFile file)
    {
        if (!Enum.IsDefined(file.ContentKind)) throw new ArgumentException("Unknown download content kind.");
        FileIntegrity.ValidateHash(file.Sha1, 40); FileIntegrity.ValidateHash(file.Sha256, 64);
        if (file.Sha1 is null && file.Sha256 is null && file.ContentKind == DownloadContentKind.Binary) throw new ArgumentException("Binary downloads require expected integrity metadata.");
        if (file.ExpectedSize < 0 || file.Sources.Count == 0) throw new ArgumentException("Invalid file plan.");
        foreach (var source in file.Sources) HttpDownloadTransport.ValidateSource(source.Uri);
        return file with { Destination = FileIntegrity.Contained(root, file.Destination) };
    }
    private static string Part(string destination) => destination + "." + Guid.NewGuid().ToString("N") + ".part";
    private void Trace(Guid id, string? source, string kind, string message)
    {
        var entry = new DownloadEvent(id, DateTimeOffset.UtcNow, source, kind, _redact?.Invoke(message)??message);
        _events.Enqueue(entry);
        if (_eventSink is not null && Volatile.Read(ref _sinkFailed) == 0)
        {
            try { _eventSink.Write(entry); }
            catch (Exception e) when (e is IOException or UnauthorizedAccessException)
            { Interlocked.Exchange(ref _sinkFailed, 1); _events.Enqueue(new(id, DateTimeOffset.UtcNow, null, "journal-error", "Detailed event journal unavailable; bounded memory diagnostics retained.")); }
        }
        while (_events.Count > 1024) _events.TryDequeue(out _);
    }
    public void Dispose() => _transport.Dispose();
}
internal sealed class DownloadFailedException(string code, string message, string? sourceId = null) : IOException(message) { internal string Code { get; } = code; internal string? SourceId { get; } = sourceId; }
