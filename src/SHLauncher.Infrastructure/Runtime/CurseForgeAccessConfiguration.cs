using System.Text.Json;
using SHLauncher.Contracts.Install;
using SHLauncher.Infrastructure.Install;
namespace SHLauncher.Infrastructure.Runtime;

// Local application-owned credential input. Never serialize this object or attach the source exception.
public sealed class CurseForgeAccessConfiguration
{
    private readonly string? _key;
    private readonly string? _problem;
    public CurseForgeAccessConfiguration(string? apiKey, string? configurationFile)
    {
        try
        {
            if (!string.IsNullOrWhiteSpace(apiKey) && !string.IsNullOrWhiteSpace(configurationFile))
                throw new InvalidDataException();
            if (!string.IsNullOrWhiteSpace(configurationFile))
            {
                if (!Path.IsPathFullyQualified(configurationFile) || configurationFile.StartsWith("\\\\",StringComparison.Ordinal) ||
                    configurationFile.StartsWith("//",StringComparison.Ordinal)) throw new InvalidDataException();
                var full = Path.GetFullPath(configurationFile);
                ArchivePathPolicy.Resolve(Path.GetDirectoryName(full)!, Path.GetFileName(full));
                using var input = new FileStream(full, FileMode.Open, FileAccess.Read, FileShare.Read);
                const int limit = 16384;
                if (input.Length is <= 0 or > limit) throw new InvalidDataException();
                var bytes = new byte[limit + 1]; var count = 0;
                while (count < bytes.Length)
                {
                    var read = input.Read(bytes, count, bytes.Length - count);
                    if (read == 0) break;
                    count += read;
                }
                if (count > limit) throw new InvalidDataException();
                var start = count >= 3 && bytes[0] == 0xef && bytes[1] == 0xbb && bytes[2] == 0xbf ? 3 : 0;
                using var json = JsonDocument.Parse(bytes.AsMemory(start, count - start), new() { MaxDepth = 4 });
                if (json.RootElement.ValueKind != JsonValueKind.Object) throw new InvalidDataException();
                var properties = json.RootElement.EnumerateObject().ToArray();
                if (properties.Length != 1 || properties[0].Name != "apiKey" || properties[0].Value.ValueKind != JsonValueKind.String)
                    throw new InvalidDataException();
                apiKey = properties[0].Value.GetString();
                if (string.IsNullOrWhiteSpace(apiKey)) throw new InvalidDataException();
            }
            if (string.IsNullOrWhiteSpace(apiKey)) return;
            if (apiKey.Length > 4096 || apiKey.Any(c => c < '!' || c > '~')) throw new InvalidDataException();
            _key = apiKey;
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException or JsonException or ArgumentException or NotSupportedException or InvalidDataException)
        {
            _problem = "CurseForge 本地访问配置无法读取或格式无效。请检查文件路径、读取权限和 apiKey 字段，修正后重启启动器；配置内容不会写入日志。";
        }
    }
    public bool HasAccess()
    {
        if (_problem is not null) throw new InstallationPreconditionException("ProviderAccessInvalid", _problem);
        return _key is not null;
    }
    public string Redact(string value)
    {
        if(_key is null)return value;
        return value.Replace(_key,"[已隐藏]",StringComparison.Ordinal)
            .Replace(Uri.EscapeDataString(_key),"[已隐藏]",StringComparison.Ordinal);
    }
    public IReadOnlyDictionary<string, string> Headers(Uri uri)
    {
        if (!uri.IsAbsoluteUri || uri.Scheme != "https" || uri.Port != 443 || uri.UserInfo.Length != 0 ||
            uri.Host is not ("api.curseforge.com" or "edge.forgecdn.net")) return new Dictionary<string, string>();
        return HasAccess() ? new Dictionary<string, string> { ["x-api-key"] = _key! } : new Dictionary<string, string>();
    }
}
