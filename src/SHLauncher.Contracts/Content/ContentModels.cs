namespace SHLauncher.Contracts.Content;
public enum ContentType { Mod, ResourcePack, Shader, Modpack }
public enum CompatibilityStatus { Compatible, IncompatibleMinecraftVersion, WrongLoader, MissingDependency, UnknownCompatibility }
public sealed record ContentSearchQuery(string Text, ContentType Type, string? MinecraftVersion, string? Loader);
public sealed record ContentDependency(string ProviderId, string ContentId, DependencyKind Kind, DependencyConstraint? Constraint = null, DependencyRole Role = DependencyRole.Ordinary);
public sealed record ContentVersion(string Id, IReadOnlyList<string> MinecraftVersions, IReadOnlyList<string> Loaders,
    IReadOnlyList<ContentDependency> Dependencies);
public sealed record ContentSearchResult(string ProviderId, string Id, string Name, ContentType Type);
public sealed record ContentCompatibility(CompatibilityStatus Status, string Explanation);
public sealed record ContentInstallRequest(string InstanceId, string ProviderId, string ContentId, string VersionId, ContentType Type);
public sealed record InstanceContentDirectories(string Mods, string ResourcePacks, string Shaders, string Saves, string Screenshots);
public interface IContentProvider
{
    string Id { get; }
    Task<IReadOnlyList<ContentSearchResult>> SearchAsync(ContentSearchQuery query, CancellationToken token);
}
public interface IContentService { InstanceContentDirectories GetDirectories(string gameDirectory); }
