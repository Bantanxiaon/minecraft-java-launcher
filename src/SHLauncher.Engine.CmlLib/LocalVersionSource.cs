using CmlLib.Core;
using CmlLib.Core.Version;
using CmlLib.Core.VersionLoader;
using CmlLib.Core.VersionMetadata;
namespace SHLauncher.Engine.CmlLib;
internal static class LocalVersionSource
{
    internal static async Task<IVersion> ReadAsync(string root, string id, CancellationToken token)
    {
        if (!Path.IsPathFullyQualified(root) || !Directory.Exists(root)) throw new DirectoryNotFoundException();
        var path = new MinecraftPath(root);
        var versions = await new LocalJsonVersionLoader(path).GetVersionMetadatasAsync(token).ConfigureAwait(false);
        var safe = new VersionMetadataCollection(versions.Where(item =>
        {
            if (item is not LocalVersionMetadata local) return false;
            try { PathSafety.EnsureContained(root, local.Path); return new FileInfo(local.Path).Length <= 8 * 1024 * 1024; }
            catch (IOException) { return false; }
        }).Select(item => new StrictLocalMetadata((LocalVersionMetadata)item)), null, null);
        return await safe.GetVersionAsync(id, token).ConfigureAwait(false);
    }
    private sealed class StrictLocalMetadata(LocalVersionMetadata source) : IVersionMetadata
    {
        public string Name => source.Name;
        public string? Type => source.Type;
        public DateTimeOffset ReleaseTime => source.ReleaseTime;
        public Task<IVersion> GetVersionAsync(CancellationToken token = default)
        { token.ThrowIfCancellationRequested(); using var input = File.OpenRead(source.Path); return Task.FromResult(StrictVersionParser.Read(input)); }
        public Task<IVersion> GetAndSaveVersionAsync(MinecraftPath path, CancellationToken token = default) => throw new NotSupportedException("Local metadata is read only.");
        public Task SaveVersionAsync(MinecraftPath path, CancellationToken token = default) => throw new NotSupportedException("Local metadata is read only.");
    }
    internal static void Dispose(IVersion version)
    {
        foreach (var item in version.EnumerateToParent()) (item as IDisposable)?.Dispose();
    }
}
