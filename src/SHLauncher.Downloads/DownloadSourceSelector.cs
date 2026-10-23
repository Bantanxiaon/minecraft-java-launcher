using SHLauncher.Contracts.Downloads;
namespace SHLauncher.Downloads;
// The provider identity and expected hash remain unchanged. Only documented identical-artifact mirrors are added.
public sealed class DownloadSourceSelector(Func<DownloadSourceMode>? mode = null, bool enableBmclApi = false) : IDownloadSourceSelector
{
    public DownloadSourceMode Mode => mode?.Invoke() ?? DownloadSourceMode.Automatic;
    public IReadOnlyList<DownloadSource> Select(DownloadFile file)
    {
        var candidates = file.Sources.ToList();
        if (enableBmclApi && file.ContentKind == DownloadContentKind.Binary && (file.Sha1 is not null || file.Sha256 is not null))
        {
            foreach (var source in file.Sources.Where(s => s.IsOfficial))
            {
                string? path = source.Uri.Host switch
                {
                    "resources.download.minecraft.net" => "/assets" + source.Uri.AbsolutePath,
                    "libraries.minecraft.net" => "/maven" + source.Uri.AbsolutePath,
                    _ => null
                };
                if (path is not null && source.Uri.Scheme == "https" && source.Uri.Query.Length == 0)
                    candidates.Add(new("BMCLAPI", new Uri("https://bmclapi2.bangbang93.com" + path), false));
            }
        }
        // Failure/Range/HTTP/integrity state is scoped by DownloadManager to each file attempt; no permanent blacklists.
        return (Mode == DownloadSourceMode.MirrorPreferred ? candidates.OrderBy(s=>s.IsOfficial) : candidates.OrderByDescending(s=>s.IsOfficial))
            .DistinctBy(s=>s.Uri.AbsoluteUri).ToArray();
    }
}
