using SHLauncher.Contracts.Engine;
namespace SHLauncher.Engine.CmlLib;
public sealed record ExistingGameFile(string Path, string? Sha1, long Size, bool Valid, string? Problem);
public static class ExistingFileInspection
{
    public static async Task<IReadOnlyList<ExistingGameFile>> InspectAsync(string root, string folderId, CancellationToken token)
    {
        return await Task.Run(async () =>
        {
            var catalog = await new CmlLibInstanceService().ScanAsync(root, token).ConfigureAwait(false);
            var instance = catalog.FirstOrDefault(x => x.Id == folderId) ?? throw new InvalidOperationException("Instance missing.");
            if (!instance.IsLaunchable) throw new LauncherEngineException(instance.Problem!);
            var version = await LocalVersionSource.ReadAsync(root, folderId, token).ConfigureAwait(false);
            try
            {
                var path = new SessionMinecraftPath(root, folderId, version.Id, root, Path.Combine(root, "unused-inspection-natives"));
                var files = await CmlLibFileVerifier.PlanAsync(path, version, token).ConfigureAwait(false);
                var result = new List<ExistingGameFile>();
                foreach (var file in files)
                {
                    string? error = null;
                    try { await CmlLibFileVerifier.CheckAsync(path, file, token).ConfigureAwait(false); }
                    catch (LauncherEngineException e) { error = e.Message; }
                    result.Add(new(file.Path!, file.Hash, file.Size, error is null, error));
                }
                return (IReadOnlyList<ExistingGameFile>)result.AsReadOnly();
            }
            finally { LocalVersionSource.Dispose(version); }
        }, token).ConfigureAwait(false);
    }
}
