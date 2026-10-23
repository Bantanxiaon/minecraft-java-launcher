using System.Security.Cryptography;
using System.Text;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Install;
namespace SHLauncher.Downloads;
public sealed class JsonMetadataSource(IDownloadManager downloads, string metadataDirectory) : IJsonMetadataSource
{
    public async Task<Stream> OpenReadAsync(Uri uri, Guid sessionId, CancellationToken token)
    {
        var key = Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(uri.AbsoluteUri)));
        var path = Path.Combine(metadataDirectory, key + "-" + Guid.NewGuid().ToString("N") + ".json");
        var result = await downloads.DownloadAsync(new(sessionId, "Official metadata", metadataDirectory,
            [new(path, null, null, null, [new(uri.Host, uri)], DownloadContentKind.JsonMetadata)]), null, token).ConfigureAwait(false);
        token.ThrowIfCancellationRequested();
        if (!result.Succeeded && uri.Scheme == "https" && uri.Host == "api.curseforge.com" && uri.Port == 443 &&
            result.Failures.Any(f => f.Code is "HTTP401" or "HTTP403"))
            throw new InstallationPreconditionException("ProviderAccessRejected", "CurseForge 拒绝了访问（401/403）。请检查 SH Launcher 自有密钥是否有效及应用权限，修正配置后重启；未替换必需文件。");
        if (!result.Succeeded) throw new IOException(result.Failures.FirstOrDefault()?.Message ?? "Metadata transfer cancelled.");
        return File.OpenRead(path);
    }
}
