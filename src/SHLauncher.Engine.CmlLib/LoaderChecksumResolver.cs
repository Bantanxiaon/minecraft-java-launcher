using SHLauncher.Contracts.Downloads;
namespace SHLauncher.Engine.CmlLib;
// Maven sidecar supplied by the artifact's own HTTPS repository, not a locally invented digest.
internal sealed class LoaderChecksumResolver(IDownloadManager downloads, string metadataDirectory)
{
    internal async Task<string> GetSha1Async(Uri artifact, Guid session, CancellationToken token)
    {
        var destination = Path.Combine(metadataDirectory, Guid.NewGuid().ToString("N") + ".sha1");
        var sidecar = new Uri(artifact.AbsoluteUri + ".sha1");
        var result = await downloads.DownloadAsync(new(session, "Library checksum", metadataDirectory,
            [new(destination, null, null, null, [new(sidecar.Host, sidecar)], DownloadContentKind.Sha1Metadata)]), null, token).ConfigureAwait(false);
        token.ThrowIfCancellationRequested();
        if (!result.Succeeded) throw new InvalidDataException("Official library checksum unavailable: " + artifact.GetLeftPart(UriPartial.Path));
        return (await File.ReadAllTextAsync(destination, token).ConfigureAwait(false)).Trim();
    }
}
