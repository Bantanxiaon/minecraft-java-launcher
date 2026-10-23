namespace SHLauncher.Contracts.Install;
public enum ModpackFormat { CurseForge, Modrinth, MultiMC, Unknown }
public enum InstallContentType { Minecraft, Loader, Mod, ResourcePack, Shader, Config, Override, Other }
public enum DestinationScope { Shared, InstanceRuntime, InstanceGame }
public sealed record PlannedArtifact(string ProviderId, string ArtifactId, DestinationScope Scope, string RelativePath,
    InstallContentType Type, bool Required, long? Size, string? Sha1, string? Sha256, IReadOnlyList<SHLauncher.Contracts.Downloads.DownloadSource> Sources,
    string? ArchiveEntry = null);
public sealed record ArchiveGameFile(string EntryName, string GamePath, long Size, string Sha256);
public sealed record ProviderFileReference(string ProviderId, long ProjectId, long FileId, bool Required);
public sealed record ModpackPreview(string ArchivePath, string ArchiveSha256, long ArchiveSize, ModpackFormat Format, string DisplayName,
    string MinecraftVersion, LoaderType Loader, string LoaderVersion, string OverridesPrefix,
    IReadOnlyList<ProviderFileReference> RemoteFiles, IReadOnlyList<ArchiveGameFile> LocalFiles, string? Problem = null);
public sealed record UnifiedInstallPlan(Guid InstanceId, string DisplayName, string MinecraftVersion, LoaderType Loader, string LoaderVersion,
    IReadOnlyList<PlannedArtifact> Artifacts, string SourceArchiveSha256);
public interface IModpackInspector { Task<ModpackPreview> InspectAsync(string archivePath, CancellationToken token); }
public interface IModpackArtifactResolver
{
    Task<IReadOnlyList<PlannedArtifact>> ResolveAsync(ModpackPreview preview, CancellationToken token);
}
