using SHLauncher.Contracts.Content;
namespace SHLauncher.Infrastructure.Content;
// Path model only. This stage never reads, installs, enables or deletes content.
public sealed class LocalContentService : IContentService
{
    public InstanceContentDirectories GetDirectories(string gameDirectory)
    {
        if (!Path.IsPathFullyQualified(gameDirectory)) throw new ArgumentException("An absolute game directory is required.", nameof(gameDirectory));
        return new(Path.Combine(gameDirectory, "mods"), Path.Combine(gameDirectory, "resourcepacks"),
            Path.Combine(gameDirectory, "shaderpacks"), Path.Combine(gameDirectory, "saves"), Path.Combine(gameDirectory, "screenshots"));
    }
}
public sealed class MockContentProvider : IContentProvider
{
    public string Id => "mock";
    public Task<IReadOnlyList<ContentSearchResult>> SearchAsync(ContentSearchQuery query, CancellationToken token)
    { token.ThrowIfCancellationRequested(); return Task.FromResult<IReadOnlyList<ContentSearchResult>>([]); }
}
