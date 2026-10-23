namespace SHLauncher.Contracts.Engine;
public enum GameLogLevel { Information, Warning, Error }
public sealed record GameLogEntry(Guid SessionId, DateTimeOffset Timestamp, GameLogLevel Level, string Source, string Message);
public interface IGameLogService
{
    void Write(Guid sessionId, GameLogLevel level, string source, string message);
    IReadOnlyList<GameLogEntry> Snapshot(Guid? sessionId = null);
}
