using System.Text.Json;
using System.Xml;
using System.Xml.Linq;
using SHLauncher.Contracts.Install;
namespace SHLauncher.Engine.CmlLib;
public sealed class ForgeVersionService(IJsonMetadataSource json, IXmlMetadataSource xml) : ILoaderVersionService
{
    public async Task<IReadOnlyList<LoaderVersionInfo>> GetVersionsAsync(LoaderType loader, string minecraftVersion, CancellationToken token)
    {
        if (loader != LoaderType.Forge) return [];
        var id = Guid.NewGuid();
        using var promotions = await json.OpenReadAsync(new("https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json"), id, token).ConfigureAwait(false);
        using var document = await JsonDocument.ParseAsync(promotions, cancellationToken: token).ConfigureAwait(false);
        var promos = document.RootElement.GetProperty("promos");
        var recommended = promos.TryGetProperty(minecraftVersion + "-recommended", out var rec) ? rec.GetString() : null;
        var latest = promos.TryGetProperty(minecraftVersion + "-latest", out var last) ? last.GetString() : null;
        using var metadata = await xml.OpenReadAsync(new("https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml"), id, token).ConfigureAwait(false);
        using var reader = XmlReader.Create(metadata, new XmlReaderSettings { Async = true, DtdProcessing = DtdProcessing.Prohibit, XmlResolver = null });
        var maven = await XDocument.LoadAsync(reader, LoadOptions.None, token).ConfigureAwait(false);
        return maven.Descendants("version").Select(v => v.Value).Where(v => v.StartsWith(minecraftVersion + "-", StringComparison.Ordinal))
            .Select(v => v[(minecraftVersion.Length + 1)..]).Distinct().Select(v => new LoaderVersionInfo(loader, minecraftVersion, v, v == recommended, v == recommended,
                v == recommended ? "Forge promotions official recommended" : v == latest ? "Forge promotions latest; no stable recommendation for this file" : "Official Maven available; no official stability marker")).ToArray();
    }
}
