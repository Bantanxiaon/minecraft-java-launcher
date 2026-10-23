using CmlLib.Core.Installer.Forge.Versions;
using SHLauncher.Contracts.Install;
namespace SHLauncher.Engine.CmlLib;
public sealed class NeoForgeVersionService(CmlLibMetadataService metadata) : ILoaderVersionService
{
    public async Task<IReadOnlyList<LoaderVersionInfo>> GetAllAsync(CancellationToken token)
    {
        using var client = metadata.CreateClient(Guid.NewGuid(), token);
        var versions = await new NeoForgeVersionLoader(client).GetNeoForgeVersions().ConfigureAwait(false);
        token.ThrowIfCancellationRequested();
        return versions.Select(v => new LoaderVersionInfo(LoaderType.NeoForge, v.MinecraftVersionName, v.NeoForgeVersionName,
            Version.TryParse(v.NeoForgeVersionName, out _), false,
            Version.TryParse(v.NeoForgeVersionName, out _) ? "NeoForge numeric release without prerelease suffix (official versioning); no official recommended flag" : "NeoForge prerelease or snapshot channel")).ToArray();
    }
    public async Task<IReadOnlyList<LoaderVersionInfo>> GetVersionsAsync(LoaderType loader, string minecraftVersion, CancellationToken token) =>
        loader != LoaderType.NeoForge ? [] : (await GetAllAsync(token).ConfigureAwait(false)).Where(v => v.MinecraftVersion == NeoForgeVersionMapper.NormalizeMinecraftVersion(minecraftVersion)).ToArray();
}
