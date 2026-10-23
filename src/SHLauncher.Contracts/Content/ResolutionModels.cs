namespace SHLauncher.Contracts.Content;
public enum DependencyKind { Required, Optional, Incompatible, Embedded, Recommended, Unknown }
public enum DependencyRole { Ordinary, Foundation }
public enum ReleaseChannel { Release, Beta, Alpha, Unknown }
public enum CompatibilityCode { Compatible, WrongMinecraftVersion, WrongLoader, LoaderVersionMismatch, MissingRequiredDependency, DependencyConflict, UnsupportedEnvironment, Unknown }
public sealed record CompatibilityResult(CompatibilityCode Code, string Reason, IReadOnlyList<string> RelatedIds);
public sealed record ContentIdentity(string ProviderId, string ProjectId);
public sealed record InstanceCompatibilityContext(string MinecraftVersion, string Loader, string? LoaderVersion);
public sealed record DependencyConstraint(string? ExactVersionId, string? MinimumVersion, string? MaximumVersion, string? UnparsedProviderConstraint = null);
public sealed record DependencyEdge(ContentIdentity Target, DependencyKind Kind, DependencyConstraint Constraint, DependencyRole Role = DependencyRole.Ordinary);
public sealed record ResolvedContentVersion(ContentIdentity Project, string VersionId, string Version, ReleaseChannel Channel,
    IReadOnlyList<string> MinecraftVersions, IReadOnlyList<string> Loaders, IReadOnlyList<DependencyEdge> Dependencies, DateTimeOffset PublishedAt);
public sealed record ContentPlanNode(ResolvedContentVersion Version, bool AlreadyInstalled, string Reason);
public sealed record ContentInstallPlan(IReadOnlyList<ContentPlanNode> Nodes, IReadOnlyList<CompatibilityResult> Problems)
{ public bool CanInstall => Problems.Count == 0; }
public interface ICompatibilityService
{
    CompatibilityResult Check(ResolvedContentVersion version, InstanceCompatibilityContext context);
}
public interface IDependencyResolver
{
    Task<ContentInstallPlan> ResolveAsync(ResolvedContentVersion root, InstanceCompatibilityContext context,
        IReadOnlyList<ResolvedContentVersion> installed, CancellationToken token);
}
public interface IContentVersionCatalog
{
    Task<IReadOnlyList<ResolvedContentVersion>> GetVersionsAsync(ContentIdentity project, CancellationToken token);
}
// Contracts only: live provider adapters, graph solving and content installation are gated to the content phase.
