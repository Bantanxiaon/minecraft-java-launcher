using System.Text.RegularExpressions;
using SHLauncher.Contracts.Engine;
namespace SHLauncher.Infrastructure.Logging;
public sealed class GameLogService(Func<string,string>? redact=null) : IGameLogService
{
    private readonly object _gate = new();
    private readonly Queue<GameLogEntry> _entries = new();
    public const int Capacity = 1024;
    public void Write(Guid sessionId, GameLogLevel level, string source, string message)
    {
        message=redact?.Invoke(message)??message;
        source=redact?.Invoke(source)??source;
        // Reject the complete line, including JSON/headers/CLI forms, before it enters any buffer.
        if (Regex.IsMatch(message, @"(?i)access.?token|refresh.?token|authorization|cookie|xsts|xbox.?token|microsoft.?token|client.?secret|api.?key|\bbearer\b", RegexOptions.CultureInvariant, TimeSpan.FromMilliseconds(50)))
            message = "[敏感信息已隐藏]";
        message = message.Length > 2048 ? message[..2048] + "…" : message;
        lock (_gate) { while (_entries.Count >= Capacity) _entries.Dequeue(); _entries.Enqueue(new(sessionId, DateTimeOffset.UtcNow, level, source, message)); }
    }
    public IReadOnlyList<GameLogEntry> Snapshot(Guid? sessionId = null)
    { lock (_gate) return _entries.Where(x => sessionId is null || x.SessionId == sessionId).ToArray(); }
}
