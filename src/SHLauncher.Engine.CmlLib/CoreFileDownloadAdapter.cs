using CmlLib.Core;
using CmlLib.Core.Files;
using CmlLib.Core.Installers;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Install;
namespace SHLauncher.Engine.CmlLib;
internal sealed class CoreFileDownloadAdapter(IDownloadManager downloads, string root, string metadataRoot, Guid session, IProgress<InstallProgress>? progress) : IGameInstaller
{
    internal long NetworkBytes { get; private set; }
    internal int LastReusedFiles { get; private set; }
    public async ValueTask Install(IEnumerable<GameFile> gameFiles, IProgress<InstallerProgressChangedEventArgs>? fileProgress,
        IProgress<ByteProgress>? byteProgress, CancellationToken token)
    {
        var plans = new List<DownloadFile>();
        foreach (var file in gameFiles.DistinctBy(f => f.Path))
        {
            PathSafety.EnsureContained(root, file.Path!);
            if (string.IsNullOrEmpty(file.Url)) continue; // locally generated output is verified after processors
            var uri = new Uri(file.Url);
            var hash = string.IsNullOrEmpty(file.Hash) ? await new LoaderChecksumResolver(downloads, metadataRoot).GetSha1Async(uri, session, token).ConfigureAwait(false) : file.Hash;
            plans.Add(new(file.Path!, file.Size > 0 ? file.Size : null, hash, null, [new(uri.Host, uri)]));
        }
        var result = await downloads.DownloadAsync(new(session, "Loader libraries", root, plans), new Forward(progress), token).ConfigureAwait(false);
        NetworkBytes += result.Files.Sum(f => f.NetworkBytes);
        LastReusedFiles = result.Files.Count(f => f.Reused);
        token.ThrowIfCancellationRequested();
        if (!result.Succeeded) throw new IOException(result.Failures.FirstOrDefault()?.Message ?? "Core file transfer failed.");
    }
    private sealed class Forward(IProgress<InstallProgress>? progress) : IProgress<DownloadTaskSnapshot>
    { public void Report(DownloadTaskSnapshot value) => progress?.Report(new("Loader 文件传输", value)); }
}
