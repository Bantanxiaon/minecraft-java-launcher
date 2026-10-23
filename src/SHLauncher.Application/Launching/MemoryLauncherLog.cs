using SHLauncher.Contracts.Engine;

namespace SHLauncher.Application.Launching;

public sealed class MemoryLauncherLog : ILauncherLog
{
    private readonly Queue<LaunchLogEntry> _entries = new();
    public IReadOnlyList<LaunchLogEntry> Entries { get { lock (_entries) return _entries.ToArray(); } }
    public void Write(LaunchLogEntry entry)
    {
        lock (_entries)
        {
            _entries.Enqueue(entry);
            while (_entries.Count > 128) _entries.Dequeue();
        }
    }
}
