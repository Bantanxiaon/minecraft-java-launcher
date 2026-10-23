using System.Collections.Concurrent;
using System.Net;
using System.Net.Sockets;
using System.Text;
internal sealed class LocalHttpServer : IAsyncDisposable
{
    private readonly TcpListener _listener = new(IPAddress.Loopback, 0);
    private readonly CancellationTokenSource _stop = new();
    private readonly ConcurrentBag<Task> _clients = new();
    private readonly ConcurrentDictionary<string, int> _calls = new();
    private readonly Task _accept;
    internal byte[] Payload { get; } = Enumerable.Range(0, 256 * 1024).Select(i => (byte)(i % 251)).ToArray();
    internal Uri BaseUri { get; }
    private int _active, _peak;
    internal int Peak => _peak;
    internal int Calls(string endpoint) => _calls.GetValueOrDefault(endpoint);
    internal LocalHttpServer()
    {
        _listener.Start(); BaseUri = new("http://127.0.0.1:" + ((IPEndPoint)_listener.LocalEndpoint).Port + "/");
        _accept = Accept();
    }
    private async Task Accept()
    {
        try { while (!_stop.IsCancellationRequested) { var client = await _listener.AcceptTcpClientAsync(_stop.Token); _clients.Add(Serve(client)); } }
        catch (OperationCanceledException) { }
    }
    private async Task Serve(TcpClient client)
    {
        using (client)
        {
            var active = Interlocked.Increment(ref _active); int old;
            do { old = _peak; if (old >= active) break; } while (Interlocked.CompareExchange(ref _peak, active, old) != old);
            try
            {
                await using var stream = client.GetStream();
                using var reader = new StreamReader(stream, Encoding.ASCII, false, 4096, true);
                var first = await reader.ReadLineAsync(_stop.Token); if (first is null) return;
                var endpoint = first.Split(' ')[1].Trim('/'); var call = _calls.AddOrUpdate(endpoint, 1, (_, n) => n + 1);
                string? line, range = null;
                while (!string.IsNullOrEmpty(line = await reader.ReadLineAsync(_stop.Token))) if (line.StartsWith("Range: ", StringComparison.OrdinalIgnoreCase)) range = line[7..];
                if (endpoint == "timeout") { await Task.Delay(2000, _stop.Token); return; }
                var code = endpoint switch { "missing" => 404, "forbidden" => 403, "unauthorized" => 401, "busy" when call == 1 => 429, "reset" when call == 1 => 502, _ => 200 };
                long start = 0, end = Payload.Length - 1;
                if (range is not null && endpoint is not ("no-range" or "slow" or "no-range-corrupt") && code == 200)
                {
                    var numbers = range[6..].Split('-'); start = long.Parse(numbers[0]); end = string.IsNullOrEmpty(numbers[1]) ? Payload.Length - 1 : long.Parse(numbers[1]);
                    code = endpoint == "range-416" ? 416 : 206;
                    if (endpoint == "range-broken" && start > 0) code = 200;
                }
                var data = endpoint is "corrupt" or "no-range-corrupt" ? Payload.Select(b => (byte)(b ^ 1)).ToArray() : Payload;
                if (endpoint == "xml") data = Encoding.UTF8.GetBytes("<metadata><version>1.2.3</version></metadata>");
                if (endpoint == "xml-dtd") data = Encoding.UTF8.GetBytes("<!DOCTYPE x [<!ENTITY ex SYSTEM 'file:///not-allowed'>]><x>&ex;</x>");
                if (endpoint == "sha1") data = Encoding.ASCII.GetBytes(Convert.ToHexString(System.Security.Cryptography.SHA1.HashData(Payload)) + "\n");
                if (endpoint == "sha1-invalid") data = Encoding.ASCII.GetBytes("<html>not a checksum</html>");
                if (code is >= 400) data = [];
                if (code != 206) { start = 0; end = data.Length - 1; }
                var size = Math.Max(0, end - start + 1);
                var headers = $"HTTP/1.1 {code} Response\r\nConnection: close\r\nContent-Length: {size}\r\n";
                if (code == 206) headers += $"Content-Range: bytes {start}-{end}/{(endpoint == "range-invalid" ? Payload.Length + 1 : Payload.Length)}\r\n";
                if (code == 429) headers += "Retry-After: 1\r\n";
                await stream.WriteAsync(Encoding.ASCII.GetBytes(headers + "\r\n"), _stop.Token);
                for (var offset = start; offset <= end; offset += 4096)
                {
                    if (endpoint == "slow") await Task.Delay(30, _stop.Token);
                    await stream.WriteAsync(data.AsMemory((int)offset, (int)Math.Min(4096, end - offset + 1)), _stop.Token);
                }
            }
            catch (Exception e) when (e is IOException or OperationCanceledException or SocketException) { }
            finally { Interlocked.Decrement(ref _active); }
        }
    }
    public async ValueTask DisposeAsync()
    { await _stop.CancelAsync(); _listener.Stop(); await _accept; await Task.WhenAll(_clients); _stop.Dispose(); }
}
