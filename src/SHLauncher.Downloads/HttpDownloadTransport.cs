using System.Net;
using System.Net.Http.Headers;
using SHLauncher.Contracts.Downloads;
namespace SHLauncher.Downloads;
internal sealed class HttpDownloadTransport(DownloadScheduler scheduler, DownloadOptions options, Func<Uri, IReadOnlyDictionary<string,string>>? authorize = null) : IDisposable
{
    private readonly HttpClient _client = new(new SocketsHttpHandler { AutomaticDecompression = DecompressionMethods.None,
        AllowAutoRedirect = false, UseCookies = false, ConnectTimeout = TimeSpan.FromSeconds(15) }) { Timeout = Timeout.InfiniteTimeSpan };
    internal async Task DownloadAsync(DownloadFile file, DownloadSource source, string part, Action<int> received,
        Action<string> trace, bool allowRanges, CancellationToken token)
    {
        if (allowRanges && options.SegmentsPerFile > 1 && file.ExpectedSize is { } length && length >= options.SegmentThreshold)
        {
            var supported = await ProbeAsync(source.Uri, length, token).ConfigureAwait(false);
            trace(supported ? "Range accepted with exact 206/Content-Range/length." : "Range unavailable; using single connection.");
            if (supported)
            {
                using var output = new FileStream(part, FileMode.CreateNew, FileAccess.Write, FileShare.ReadWrite, 81920, FileOptions.Asynchronous | FileOptions.RandomAccess);
                output.SetLength(length);
                using var stop = CancellationTokenSource.CreateLinkedTokenSource(token);
                try
                {
                    var count = Math.Min(options.SegmentsPerFile, 4);
                    await Task.WhenAll(Enumerable.Range(0, count).Select(async segment =>
                    {
                        var start = length * segment / count; var end = length * (segment + 1) / count - 1;
                        try { await TransferAsync(source.Uri, output, start, end, length, received, stop.Token, trace: trace).ConfigureAwait(false); }
                        catch { await stop.CancelAsync().ConfigureAwait(false); throw; }
                    })).ConfigureAwait(false);
                    await output.FlushAsync(token).ConfigureAwait(false); output.Flush(true); return;
                }
                catch (RangeProtocolException) { throw; }
            }
        }
        using var stream = new FileStream(part, FileMode.CreateNew, FileAccess.Write, FileShare.Read, 81920, FileOptions.Asynchronous);
        await TransferAsync(source.Uri, stream, null, null, file.ExpectedSize, received, token, file.ContentKind == DownloadContentKind.Sha1Metadata ? 1024 : null, trace).ConfigureAwait(false);
        await stream.FlushAsync(token).ConfigureAwait(false); stream.Flush(true);
    }
    private async Task<bool> ProbeAsync(Uri uri, long length, CancellationToken token)
    {
        await scheduler.AcquireConnectionAsync(token).ConfigureAwait(false);
        try
        {
            using var timeout = CancellationTokenSource.CreateLinkedTokenSource(token); timeout.CancelAfter(options.Timeout);
            using var response = await SendAsync(uri, new RangeHeaderValue(0, 0), timeout.Token).ConfigureAwait(false);
            if (response.StatusCode is HttpStatusCode.OK or HttpStatusCode.RequestedRangeNotSatisfiable) return false;
            CheckStatus(response);
            var range = response.Content.Headers.ContentRange;
            return response.StatusCode == HttpStatusCode.PartialContent && range is { Unit: "bytes", From: 0, To: 0 } && range.Length == length && response.Content.Headers.ContentLength == 1;
        }
        finally { scheduler.ReleaseConnection(); }
    }
    private async Task TransferAsync(Uri uri, FileStream output, long? start, long? end, long? total, Action<int> received, CancellationToken token, long? unknownSizeBudget = null, Action<string>? trace = null)
    {
        var maxUnknownSize = unknownSizeBudget ?? options.MaxUnknownSize;
        await scheduler.AcquireConnectionAsync(token).ConfigureAwait(false);
        try
        {
            using var timeout = CancellationTokenSource.CreateLinkedTokenSource(token); timeout.CancelAfter(options.Timeout);
            using var response = await SendAsync(uri, start is null ? null : new RangeHeaderValue(start, end), timeout.Token, trace).ConfigureAwait(false);
            if (start is not null && response.StatusCode is HttpStatusCode.OK or HttpStatusCode.RequestedRangeNotSatisfiable)
                throw new RangeProtocolException();
            CheckStatus(response);
            var count = start is null ? total ?? response.Content.Headers.ContentLength : end - start + 1;
            if (start is not null)
            {
                var range = response.Content.Headers.ContentRange;
                if (response.StatusCode != HttpStatusCode.PartialContent || range?.Unit != "bytes" || range.From != start || range.To != end || range.Length != total || response.Content.Headers.ContentLength != count) throw new RangeProtocolException();
            }
            else if (response.StatusCode != HttpStatusCode.OK || response.Content.Headers.ContentRange is not null) throw new InvalidDataException("Unexpected single-response status or Content-Range.");
            if (response.Content.Headers.ContentEncoding.Count != 0) throw new InvalidDataException("Encoded binary response rejected.");
            if (start is null && total is not null && response.Content.Headers.ContentLength is { } announced && announced != total) throw new InvalidDataException("Response size does not match metadata.");
            if (count is > 0 && total is null && count > maxUnknownSize) throw new InvalidDataException("Unverified response exceeds metadata size budget.");
            await using var input = await response.Content.ReadAsStreamAsync(timeout.Token).ConfigureAwait(false);
            var buffer = new byte[81920]; long done = 0;
            while (true)
            {
                timeout.CancelAfter(options.Timeout);
                var read = await input.ReadAsync(buffer, timeout.Token).ConfigureAwait(false);
                if (read == 0) break;
                done += read;
                if (count is { } expected && done > expected || total is null && done > maxUnknownSize) throw new InvalidDataException("Response exceeds its declared budget.");
                await RandomAccess.WriteAsync(output.SafeFileHandle, buffer.AsMemory(0, read), (start ?? 0) + done - read, timeout.Token).ConfigureAwait(false);
                received(read);
            }
            if (count is { } required && done != required) throw new InvalidDataException("Truncated response.");
        }
        finally { scheduler.ReleaseConnection(); }
    }
    private async Task<HttpResponseMessage> SendAsync(Uri uri, RangeHeaderValue? range, CancellationToken token, Action<string>? trace = null)
    {
        for (var redirects = 0; ; redirects++)
        {
            ValidateSource(uri);
            using var request = new HttpRequestMessage(HttpMethod.Get, uri); request.Headers.Range = range; request.Headers.UserAgent.ParseAdd("SHLauncherNext/0.3");
            foreach (var header in authorize?.Invoke(uri) ?? new Dictionary<string,string>())
                if (!request.Headers.TryAddWithoutValidation(header.Key,header.Value)) throw new InvalidDataException("Unsupported request authorization header.");
            var response = await _client.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, token).ConfigureAwait(false);
            if ((int)response.StatusCode is not (301 or 302 or 303 or 307 or 308))
            { trace?.Invoke("Response " + (int)response.StatusCode + " from " + uri.GetLeftPart(UriPartial.Path)); return response; }
            var location = response.Headers.Location; response.Dispose();
            if (location is null || redirects >= 5) throw new HttpRequestException("Invalid redirect chain.");
            var next = location.IsAbsoluteUri ? location : new Uri(uri, location);
            if (uri.Scheme == "https" && next.Scheme != "https") throw new HttpRequestException("HTTPS downgrade rejected.");
            try { ValidateSource(next); } catch(ArgumentException error) { throw new InvalidDataException("Invalid redirect source.",error); }
            trace?.Invoke("Redirect to " + next.GetLeftPart(UriPartial.Path));
            uri = next;
        }
    }
    internal static void ValidateSource(Uri uri)
    { if (!uri.IsAbsoluteUri || uri.UserInfo.Length != 0 || !(uri.Scheme == "https" || uri.Scheme == "http" && uri.IsLoopback)) throw new ArgumentException("Only HTTPS sources or local HTTP test servers are allowed."); }
    private static void CheckStatus(HttpResponseMessage response)
    {
        if (response.IsSuccessStatusCode) return;
        var delay = response.Headers.RetryAfter?.Delta ?? (response.Headers.RetryAfter?.Date - DateTimeOffset.UtcNow);
        throw new SourceHttpException((int)response.StatusCode, delay);
    }
    public void Dispose() => _client.Dispose();
}
internal sealed class SourceHttpException(int status, TimeSpan? retryAfter) : HttpRequestException("HTTP " + status)
{ internal int Status { get; } = status; internal TimeSpan? RetryAfter { get; } = retryAfter; }
internal sealed class RangeProtocolException : IOException;
