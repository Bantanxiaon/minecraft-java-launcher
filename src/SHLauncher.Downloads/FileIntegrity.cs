using System.Security.Cryptography;
using SHLauncher.Contracts.Downloads;
namespace SHLauncher.Downloads;
internal static class FileIntegrity
{
    internal static async Task<bool> VerifyAsync(string path, DownloadFile file, CancellationToken token, Action<string>? diagnostic = null)
    {
        if (!File.Exists(path)) return false;
        if (file.ExpectedSize is { } size && new FileInfo(path).Length != size) return false;
        if (file.Sha1 is not null)
        {
            using var stream = File.OpenRead(path);
            var actual = Convert.ToHexString(await SHA1.HashDataAsync(stream, token).ConfigureAwait(false));
            diagnostic?.Invoke("SHA1 expected=" + file.Sha1 + "; obtained=" + actual);
            if (!actual.Equals(file.Sha1, StringComparison.OrdinalIgnoreCase)) return false;
        }
        if (file.Sha256 is not null)
        {
            using var stream = File.OpenRead(path);
            var actual = Convert.ToHexString(await SHA256.HashDataAsync(stream, token).ConfigureAwait(false));
            diagnostic?.Invoke("SHA256 expected=" + file.Sha256 + "; obtained=" + actual);
            if (!actual.Equals(file.Sha256, StringComparison.OrdinalIgnoreCase)) return false;
        }
        if (file.ContentKind == DownloadContentKind.JsonMetadata)
        {
            try { using var stream = File.OpenRead(path); using var document = await System.Text.Json.JsonDocument.ParseAsync(stream, cancellationToken: token).ConfigureAwait(false); }
            catch (System.Text.Json.JsonException) { return false; }
        }
        if (file.ContentKind == DownloadContentKind.Sha1Metadata)
        {
            if (new FileInfo(path).Length > 1024) return false;
            var checksum = (await File.ReadAllTextAsync(path, token).ConfigureAwait(false)).Trim();
            if (checksum.Length != 40 || checksum.Any(c => !Uri.IsHexDigit(c))) return false;
        }
        if (file.ContentKind == DownloadContentKind.XmlMetadata)
        {
            try
            {
                using var input = System.Xml.XmlReader.Create(path, new System.Xml.XmlReaderSettings {
                    Async = true, DtdProcessing = System.Xml.DtdProcessing.Prohibit, XmlResolver = null, MaxCharactersInDocument = 64 * 1024 * 1024 });
                while (await input.ReadAsync().ConfigureAwait(false)) token.ThrowIfCancellationRequested();
            }
            catch (System.Xml.XmlException) { return false; }
        }
        return true;
    }
    internal static string Contained(string root, string target)
    {
        if (!Path.IsPathFullyQualified(root) || !Path.IsPathFullyQualified(target)) throw new ArgumentException("Absolute paths required.");
        var path = Path.GetFullPath(target); var boundary = Path.GetFullPath(root).TrimEnd(Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar;
        if (!path.StartsWith(boundary, OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal)) throw new ArgumentException("Destination escapes its allowed root.");
        for (FileSystemInfo? item = new FileInfo(path); item is not null; item = item is DirectoryInfo d ? d.Parent : ((FileInfo)item).Directory)
        {
            if (IsLinked(item.FullName)) throw new ArgumentException("Linked download destinations are not supported.");
        }
        return path;
    }
    internal static void ValidateHash(string? hash, int length)
    { if (hash is not null && (hash.Length != length || hash.Any(c => !Uri.IsHexDigit(c)))) throw new ArgumentException("Invalid expected hash."); }
    internal static string? CacheKey(DownloadFile file) => file.Sha256 is not null ? "sha256-" + file.Sha256.ToLowerInvariant() : file.Sha1 is not null ? "sha1-" + file.Sha1.ToLowerInvariant() : null;
    private static bool IsLinked(string path)
    {
        try{return (File.GetAttributes(path)&FileAttributes.ReparsePoint)!=0;}
        catch(FileNotFoundException){return false;}
        catch(DirectoryNotFoundException){return false;}
    }

}
