using System.Runtime.CompilerServices;
using SHLauncher.Contracts.Engine;
namespace SHLauncher.Bootstrap;
internal sealed class InstanceAccess
{
    private readonly SemaphoreSlim[] _locks = Enumerable.Range(0,64).Select(_ => new SemaphoreSlim(1)).ToArray();
    internal SemaphoreSlim For(string root)
    {
        var path = Path.GetFullPath(root).TrimEnd(Path.DirectorySeparatorChar);
        var hash = (OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal).GetHashCode(path);
        return _locks[(hash & int.MaxValue) % _locks.Length];
    }
    internal ILauncherEngine Guard(ILauncherEngine engine, string root, Action? beforeLaunch = null) => new Guarded(engine, For(root),beforeLaunch);
    private sealed class Guarded(ILauncherEngine engine, SemaphoreSlim gate,Action? beforeLaunch) : ILauncherEngine
    {
        public Task<IReadOnlyList<InstanceInfo>> GetInstancesAsync(CancellationToken token) => engine.GetInstancesAsync(token);
        public async IAsyncEnumerable<EngineUpdate> LaunchAsync(LaunchRequest request, [EnumeratorCancellation] CancellationToken token)
        {
            await gate.WaitAsync(token).ConfigureAwait(false);
            try { beforeLaunch?.Invoke(); await foreach (var update in engine.LaunchAsync(request,token).ConfigureAwait(false)) yield return update; }
            finally { gate.Release(); }
        }
    }
}
