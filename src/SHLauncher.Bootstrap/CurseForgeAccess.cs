using SHLauncher.Infrastructure.Runtime;
namespace SHLauncher.Bootstrap;
public sealed partial class LauncherComposition
{
    private static readonly Lazy<(string? Key, string? File)> OwnCurseForgeInput = new(() =>
    {
        var key = Environment.GetEnvironmentVariable("SH_CURSEFORGE_API_KEY");
        var file = Environment.GetEnvironmentVariable("SH_CURSEFORGE_CONFIG_FILE");
        // Consume only process startup settings. Neither the key nor its file path reaches Java/processors.
        Environment.SetEnvironmentVariable("SH_CURSEFORGE_API_KEY", null, EnvironmentVariableTarget.Process);
        Environment.SetEnvironmentVariable("SH_CURSEFORGE_CONFIG_FILE", null, EnvironmentVariableTarget.Process);
        return (key, file);
    });
    private CurseForgeAccessConfiguration _curseForgeAccess = null!;
    private void InitializeCurseForgeAccess()
    {
        var (key, file) = OwnCurseForgeInput.Value;
        if (string.IsNullOrWhiteSpace(key) && string.IsNullOrWhiteSpace(file))
        {
            var local = Path.Combine(_workspace, "settings", "curseforge.local.json");
            if (File.Exists(local)) file = local;
        }
        _curseForgeAccess = new(key, file);
        GameLogs=new SHLauncher.Infrastructure.Logging.GameLogService(_curseForgeAccess.Redact);
    }
    private bool HasCurseForgeAccess() => _curseForgeAccess.HasAccess();
    private IReadOnlyDictionary<string, string> CurseForgeHeaders(Uri uri) => _curseForgeAccess.Headers(uri);
    public string RedactProviderSecrets(string value)=>_curseForgeAccess.Redact(value);
    public async Task VerifyCurseForgeAccessAsync(CancellationToken token)
    {
        if(!HasCurseForgeAccess())throw new SHLauncher.Contracts.Install.InstallationPreconditionException("ProviderAccessMissing","尚未配置 SH Launcher 自有 CurseForge 访问。");
        var source=new SHLauncher.Downloads.JsonMetadataSource(_downloads,Path.Combine(_workspace,"shared","metadata"));
        using var stream=await source.OpenReadAsync(new Uri("https://api.curseforge.com/v1/categories?gameId=432&classesOnly=true"),Guid.NewGuid(),token).ConfigureAwait(false);
        using var json=await System.Text.Json.JsonDocument.ParseAsync(stream,cancellationToken:token).ConfigureAwait(false);
        if(!json.RootElement.TryGetProperty("data",out var categories)||categories.ValueKind!=System.Text.Json.JsonValueKind.Array||categories.GetArrayLength()==0)
            throw new InvalidDataException("CurseForge authentication probe returned unexpected category metadata.");
    }
}
