using CmlLib.Core.ModLoaders.QuiltMC;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Install;
namespace SHLauncher.Engine.CmlLib;
public sealed class QuiltInstallAdapter(IDownloadManager downloads, CmlLibMetadataService metadata, string allowedInstancesRoot, string metadataDirectory)
    : ILoaderVersionService, ILoaderInstallService
{
    public async Task<IReadOnlyCollection<string>> GetSupportedMinecraftAsync(CancellationToken token)
    {
        using var client = metadata.CreateClient(Guid.NewGuid(), token);
        return await new QuiltInstaller(client).GetSupportedVersionNames().ConfigureAwait(false);
    }
    public async Task<IReadOnlyList<LoaderVersionInfo>> GetVersionsAsync(LoaderType loader, string minecraftVersion, CancellationToken token)
    {
        if (loader != LoaderType.Quilt || !(await GetSupportedMinecraftAsync(token).ConfigureAwait(false)).Contains(minecraftVersion)) return [];
        using var client = metadata.CreateClient(Guid.NewGuid(), token);
        var versions = await new QuiltInstaller(client).GetLoaders(minecraftVersion).ConfigureAwait(false); token.ThrowIfCancellationRequested();
        // Quilt API does not publish loader stable; do not use the core model's default true.
        return versions.Where(v => !string.IsNullOrEmpty(v.Version)).Select(v => new LoaderVersionInfo(loader, minecraftVersion, v.Version!,
            System.Version.TryParse(v.Version, out _), false,
            "Quilt official installer release filter: no '-' prerelease suffix; no official recommended/stable flag in API")).ToArray();
    }
    public async Task<InstallResult> InstallAsync(LoaderInstallRequest request, IProgress<InstallProgress>? progress, CancellationToken token)
    {
        if (request.Loader != LoaderType.Quilt) throw new NotSupportedException();
        var choices = await GetVersionsAsync(request.Loader, request.MinecraftVersion, token).ConfigureAwait(false);
        if (!choices.Any(v => v.Version == request.LoaderVersion && v.IsStable)) throw new InvalidDataException("Select a non-beta Quilt loader from the selected Minecraft metadata.");
        using var client = metadata.CreateClient(request.Id, token); var installer = new QuiltInstaller(client);
        return await new ProfileLoaderInstallPipeline(downloads, metadata, allowedInstancesRoot, metadataDirectory)
            .InstallAsync(request, () => installer.GetProfileJson(request.MinecraftVersion, request.LoaderVersion), QuiltInstaller.GetVersionName(request.MinecraftVersion, request.LoaderVersion), progress, token).ConfigureAwait(false);
    }
}
