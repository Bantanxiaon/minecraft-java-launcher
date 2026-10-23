using System.Security.Cryptography;
using System.Text;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Install;
namespace SHLauncher.Downloads;
public sealed class XmlMetadataSource(IDownloadManager downloads, string metadataDirectory) : IXmlMetadataSource
{
    public async Task<Stream> OpenReadAsync(Uri uri, Guid sessionId, CancellationToken token)
    {
        var key = Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(uri.AbsoluteUri)));
        var path = Path.Combine(metadataDirectory, key + "-" + Guid.NewGuid().ToString("N") + ".xml");
        var result = await downloads.DownloadAsync(new(sessionId, "Official metadata", metadataDirectory,
            [new(path, null, null, null, [new(uri.Host, uri)], DownloadContentKind.XmlMetadata)]), null, token).ConfigureAwait(false);
        token.ThrowIfCancellationRequested();
        if (!result.Succeeded) throw new IOException(result.Failures.FirstOrDefault()?.Message ?? "Metadata transfer cancelled.");
        return File.OpenRead(path);
    }
}
