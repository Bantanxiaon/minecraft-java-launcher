using System.Net;
using CmlLib.Core.VersionLoader;
using CmlLib.Core.VersionMetadata;
using SHLauncher.Contracts.Install;
namespace SHLauncher.Engine.CmlLib;
public sealed class CmlLibMetadataService(IJsonMetadataSource metadata) : IMinecraftVersionService
{
    // Official endpoint; parsing and model definitions belong to the pinned mature core.
    internal async Task<JsonVersionManifestModel> ManifestAsync(Guid id, CancellationToken token)
    {
        using var client = CreateClient(id, token);
        var loader = new MojangJsonVersionLoader(client, "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json");
        return await loader.GetManifestAsync(token).ConfigureAwait(false) ?? throw new InvalidDataException("Empty official manifest.");
    }
    public async Task<IReadOnlyList<MinecraftVersionInfo>> GetVersionsAsync(CancellationToken token)
    {
        var manifest = await ManifestAsync(Guid.NewGuid(), token).ConfigureAwait(false);
        return manifest.Versions.Select(v => new MinecraftVersionInfo(v.Id ?? "", v.Type switch
        { "release" => MinecraftVersionKind.Release, "snapshot" => MinecraftVersionKind.Snapshot, "old_beta" => MinecraftVersionKind.OldBeta,
            "old_alpha" => MinecraftVersionKind.OldAlpha, _ => MinecraftVersionKind.Special }, v.ReleaseTime, v.Id == manifest.Latest?.Release))
            .Where(v => v.Id.Length > 0).ToArray();
    }
    internal HttpClient CreateClient(Guid id, CancellationToken token) => new(new MetadataBridge(metadata, id, token));
    // This handler has no sockets. All requests are delegated to the shared download manager through an own contract.
    private sealed class MetadataBridge(IJsonMetadataSource source, Guid sessionId, CancellationToken operationToken) : HttpMessageHandler
    {
        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken token)
        {
            if (request.Method != HttpMethod.Get || request.RequestUri is null) throw new NotSupportedException("Only metadata GET is supported.");
            using var linked = CancellationTokenSource.CreateLinkedTokenSource(token, operationToken);
            var stream = await source.OpenReadAsync(request.RequestUri, sessionId, linked.Token).ConfigureAwait(false);
            return new(HttpStatusCode.OK) { Content = new StreamContent(stream), RequestMessage = request };
        }
    }
}
