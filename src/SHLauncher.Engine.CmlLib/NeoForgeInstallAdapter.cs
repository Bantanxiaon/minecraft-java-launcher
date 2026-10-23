using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Engine;
using SHLauncher.Contracts.Install;
using SHLauncher.Contracts.Runtime;
namespace SHLauncher.Engine.CmlLib;
public sealed class NeoForgeInstallAdapter(IDownloadManager downloads, CmlLibMetadataService metadata, NeoForgeVersionService versions,
    string allowedInstancesRoot, string metadataDirectory, string stagingRoot, IJavaRuntimeService java, IJavaSelectionService selection,
    IReadOnlyList<string> javaCandidates, IGameLogService processorLogs, string? manualJava = null) : ILoaderInstallService
{
    public async Task<InstallResult> InstallAsync(LoaderInstallRequest request, IProgress<InstallProgress>? progress, CancellationToken token)
    {
        if (request.Loader != LoaderType.NeoForge) throw new NotSupportedException();
        var candidates = await versions.GetVersionsAsync(request.Loader, request.MinecraftVersion, token).ConfigureAwait(false);
        if (!candidates.Any(v => v.Version == request.LoaderVersion && v.IsStable)) throw new InvalidDataException("Select a stable NeoForge release explicitly supporting the selected Minecraft version.");
        var value = request.LoaderVersion;
        var uri = new Uri("https://maven.neoforged.net/releases/net/neoforged/neoforge/" + value + "/neoforge-" + value + "-installer.jar");
        return await new ModernLoaderInstallPipeline(downloads, metadata, allowedInstancesRoot, metadataDirectory, stagingRoot, java, selection, javaCandidates, processorLogs, manualJava)
            .InstallAsync(request, uri, progress, token).ConfigureAwait(false);
    }
}
