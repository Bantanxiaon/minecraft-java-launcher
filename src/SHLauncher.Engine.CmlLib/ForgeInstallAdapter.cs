using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Engine;
using SHLauncher.Contracts.Install;
using SHLauncher.Contracts.Runtime;
namespace SHLauncher.Engine.CmlLib;
public sealed class ForgeInstallAdapter(IDownloadManager downloads, CmlLibMetadataService metadata, ILoaderVersionService versions,
    string allowedInstancesRoot, string metadataDirectory, string stagingRoot, IJavaRuntimeService java, IJavaSelectionService selection,
    IReadOnlyList<string> javaCandidates, IGameLogService processorLogs, string? manualJava = null) : ILoaderInstallService
{
    public async Task<InstallResult> InstallAsync(LoaderInstallRequest request, IProgress<InstallProgress>? progress, CancellationToken token)
    {
        if (request.Loader != LoaderType.Forge) throw new NotSupportedException();
        var candidates = await versions.GetVersionsAsync(request.Loader, request.MinecraftVersion, token).ConfigureAwait(false);
        if (!candidates.Any(v => v.Version == request.LoaderVersion && v.MinecraftVersion == request.MinecraftVersion)) throw new InvalidDataException("The exact Forge build is not present in official metadata for this Minecraft version.");
        var artifact = request.MinecraftVersion + "-" + request.LoaderVersion;
        var uri = new Uri("https://maven.minecraftforge.net/net/minecraftforge/forge/" + artifact + "/forge-" + artifact + "-installer.jar");
        return await new ModernLoaderInstallPipeline(downloads, metadata, allowedInstancesRoot, metadataDirectory, stagingRoot, java, selection, javaCandidates, processorLogs, manualJava)
            .InstallAsync(request, uri, progress, token).ConfigureAwait(false);
    }
}
