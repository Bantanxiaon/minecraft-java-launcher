using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Engine;
using SHLauncher.Contracts.Install;
using SHLauncher.Contracts.Runtime;
namespace SHLauncher.Engine.CmlLib;
public sealed class CmlLibInstallationBackend : IInstallationBackend, IMinecraftVersionService, ILoaderVersionService
{
    private readonly string _instances;
    private readonly CmlLibMetadataService _metadata;
    private readonly IVanillaInstallService _vanilla;
    private readonly Dictionary<LoaderType, ILoaderVersionService> _versions;
    private readonly Dictionary<LoaderType, ILoaderInstallService> _installers;
    public CmlLibInstallationBackend(IDownloadManager downloads, IJsonMetadataSource json, IXmlMetadataSource xml, string instancesRoot,
        string metadataRoot, string stagingRoot, IJavaRuntimeService java, IJavaSelectionService selection, IReadOnlyList<string> javaCandidates, IGameLogService logs, string? manualJava = null)
    {
        _instances = instancesRoot; _metadata = new(json); _vanilla = new CmlLibVanillaInstallService(downloads, _metadata, instancesRoot);
        var fabric = new FabricInstallAdapter(downloads, _metadata, instancesRoot, metadataRoot);
        var quilt = new QuiltInstallAdapter(downloads, _metadata, instancesRoot, metadataRoot);
        var forgeVersions = new ForgeVersionService(json, xml); var neoVersions = new NeoForgeVersionService(_metadata);
        _versions = new() { [LoaderType.Fabric] = fabric, [LoaderType.Forge] = forgeVersions, [LoaderType.NeoForge] = neoVersions, [LoaderType.Quilt] = quilt };
        _installers = new() { [LoaderType.Fabric] = fabric, [LoaderType.Quilt] = quilt,
            [LoaderType.Forge] = new ForgeInstallAdapter(downloads, _metadata, forgeVersions, instancesRoot, metadataRoot, stagingRoot, java, selection, javaCandidates, logs, manualJava),
            [LoaderType.NeoForge] = new NeoForgeInstallAdapter(downloads, _metadata, neoVersions, instancesRoot, metadataRoot, stagingRoot, java, selection, javaCandidates, logs, manualJava) };
    }
    public Task<IReadOnlyList<MinecraftVersionInfo>> GetVersionsAsync(CancellationToken token) => _metadata.GetVersionsAsync(token);
    public Task<IReadOnlyList<LoaderVersionInfo>> GetVersionsAsync(LoaderType loader, string minecraftVersion, CancellationToken token) =>
        _versions.TryGetValue(loader, out var service) ? service.GetVersionsAsync(loader, minecraftVersion, token) : Task.FromResult<IReadOnlyList<LoaderVersionInfo>>([]);
    public Task<InstallResult> InstallAsync(InstallationRecipe recipe, IProgress<InstallProgress>? progress, CancellationToken token)
    {
        if (recipe.InstanceKey == Guid.Empty) throw new ArgumentException("Instance identity is required.");
        var destination = PathSafety.EnsureContained(_instances, Path.Combine(_instances, recipe.InstanceKey.ToString("N"), "runtime"));
        return recipe.Loader == LoaderType.Vanilla ? _vanilla.InstallAsync(new(recipe.Id, recipe.MinecraftVersion, destination), progress, token) :
            _installers.TryGetValue(recipe.Loader, out var service) ? service.InstallAsync(new(recipe.Id, recipe.MinecraftVersion, destination, recipe.Loader, recipe.LoaderVersion), progress, token) : throw new NotSupportedException("Unsupported loader.");
    }
}
