using SHLauncher.Contracts.Install;
namespace SHLauncher.Application.Versions;
public interface IVersionResolver
{
    IReadOnlyList<MinecraftVersionInfo> Minecraft(IEnumerable<MinecraftVersionInfo> versions, MinecraftVersionKind? kind);
    IReadOnlyList<LoaderVersionInfo> Loaders(IEnumerable<LoaderVersionInfo> versions, LoaderType loader, string minecraft);
    LoaderVersionInfo? Preferred(IEnumerable<LoaderVersionInfo> versions);
}
public sealed class VersionResolver : IVersionResolver
{
    public IReadOnlyList<MinecraftVersionInfo> Minecraft(IEnumerable<MinecraftVersionInfo> versions, MinecraftVersionKind? kind) => MinecraftVersionResolver.List(versions, kind);
    public IReadOnlyList<LoaderVersionInfo> Loaders(IEnumerable<LoaderVersionInfo> versions, LoaderType loader, string minecraft) =>
        versions.Where(v => v.Loader == loader && v.MinecraftVersion == minecraft && v.IsStable)
        .OrderByDescending(v => v.IsRecommended).ThenByDescending(v => Version.TryParse(v.Version, out var parsed) ? parsed : new Version(0,0)).ToArray();
    public LoaderVersionInfo? Preferred(IEnumerable<LoaderVersionInfo> versions) => versions.FirstOrDefault(v => v.IsRecommended) ?? versions.FirstOrDefault();
}
public sealed class VersionSelectionService(IMinecraftVersionService minecraft, ILoaderVersionService loaders, IVersionResolver resolver)
{
    public Task<IReadOnlyList<MinecraftVersionInfo>> FetchMinecraftAsync(CancellationToken token) => minecraft.GetVersionsAsync(token);
    public IReadOnlyList<MinecraftVersionInfo> Filter(IEnumerable<MinecraftVersionInfo> items, MinecraftVersionKind? kind) => resolver.Minecraft(items, kind);
    public async Task<IReadOnlyList<LoaderVersionInfo>> FetchLoadersAsync(LoaderType loader, string version, CancellationToken token) =>
        resolver.Loaders(await loaders.GetVersionsAsync(loader, version, token).ConfigureAwait(false), loader, version);
    public LoaderVersionInfo? Preferred(IEnumerable<LoaderVersionInfo> versions) => resolver.Preferred(versions);
}
