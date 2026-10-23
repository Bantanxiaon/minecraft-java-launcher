using System.Text.Json;
using SHLauncher.Contracts.Downloads;
namespace SHLauncher.Infrastructure.Runtime;
public sealed class DownloadSourcePreferences : IDownloadSourcePreferences
{
    private readonly string _path;
    private readonly SemaphoreSlim _gate = new(1);
    private int _mode;
    public DownloadSourceMode Mode => (DownloadSourceMode)Volatile.Read(ref _mode);
    public DownloadSourcePreferences(string path)
    {
        _path = path;
        if (File.Exists(path))
        {
            if (new FileInfo(path).Length > 4096) throw new IOException("Download preferences exceed size limit.");
            try { var value = JsonSerializer.Deserialize<Preference>(File.ReadAllText(path)); if (value is null || !Enum.IsDefined(value.Mode)) throw new JsonException("Unknown source mode."); _mode=(int)value.Mode; }
            catch (JsonException error) { throw new IOException("Download preferences invalid; original retained.", error); }
        }
    }
    public async Task SetModeAsync(DownloadSourceMode mode,CancellationToken token)
    {
        if (!Enum.IsDefined(mode)) throw new ArgumentException("Unknown source mode.");
        await _gate.WaitAsync(token).ConfigureAwait(false);
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(_path)!);
            var part = _path + "." + Guid.NewGuid().ToString("N") + ".part";
            await using (var stream = new FileStream(part,FileMode.CreateNew,FileAccess.Write,FileShare.None,4096,true))
            { await JsonSerializer.SerializeAsync(stream,new Preference(mode),cancellationToken:token).ConfigureAwait(false); await stream.FlushAsync(token).ConfigureAwait(false);stream.Flush(true); }
            token.ThrowIfCancellationRequested(); File.Move(part,_path,true); Volatile.Write(ref _mode,(int)mode);
        }
        finally { _gate.Release(); }
    }
    private sealed record Preference(DownloadSourceMode Mode);
}
