using CmlLib.Core.ModLoaders.FabricMC;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Install;
namespace SHLauncher.Engine.CmlLib;
public sealed class FabricInstallAdapter(IDownloadManager downloads, CmlLibMetadataService metadata, string allowedInstancesRoot, string metadataDirectory)
    : ILoaderVersionService, ILoaderInstallService
{
    public async Task<IReadOnlyList<LoaderVersionInfo>> GetVersionsAsync(LoaderType loader, string minecraftVersion, CancellationToken token)
    {
        if (loader != LoaderType.Fabric) return [];
        using var client = metadata.CreateClient(Guid.NewGuid(), token); var installer = new FabricInstaller(client);
        if (!(await installer.GetSupportedVersionNames().ConfigureAwait(false)).Contains(minecraftVersion)) return [];
        var versions = await installer.GetLoaders(minecraftVersion).ConfigureAwait(false); token.ThrowIfCancellationRequested();
        return versions.Where(v => !string.IsNullOrEmpty(v.Version)).Select(v => new LoaderVersionInfo(loader, minecraftVersion, v.Version!, v.Stable, false,
            v.Stable ? "Fabric metadata stable=true; official recommendation not provided" : "Fabric metadata stable=false")).ToArray();
    }
    public async Task<InstallResult> InstallAsync(LoaderInstallRequest request, IProgress<InstallProgress>? progress, CancellationToken token)
    {
        if (request.Loader != LoaderType.Fabric) throw new NotSupportedException();
        var choices = await GetVersionsAsync(request.Loader, request.MinecraftVersion, token).ConfigureAwait(false);
        if (!choices.Any(v => v.Version == request.LoaderVersion && v.IsStable)) throw new InvalidDataException("Select a Fabric stable loader explicitly supported for this Minecraft release.");
        using var client = metadata.CreateClient(request.Id, token); var installer = new FabricInstaller(client);
        return await new ProfileLoaderInstallPipeline(downloads, metadata, allowedInstancesRoot, metadataDirectory)
            .InstallAsync(request, () => installer.GetProfileJson(request.MinecraftVersion, request.LoaderVersion), FabricInstaller.GetVersionName(request.MinecraftVersion, request.LoaderVersion), progress, token).ConfigureAwait(false);
    }
}
