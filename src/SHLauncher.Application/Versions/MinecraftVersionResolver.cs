using SHLauncher.Contracts.Install;
namespace SHLauncher.Application.Versions;
public static class MinecraftVersionResolver
{
    public static IReadOnlyList<MinecraftVersionInfo> List(IEnumerable<MinecraftVersionInfo> versions, MinecraftVersionKind? kind = MinecraftVersionKind.Release) =>
        versions.Where(v => kind is null || v.Kind == kind).OrderByDescending(v => v, new VersionOrder()).ToArray();
    public static MinecraftVersionInfo? Recommended(IEnumerable<MinecraftVersionInfo> versions) => versions.SingleOrDefault(v => v.Kind == MinecraftVersionKind.Release && v.OfficialLatestRelease);
    private sealed class VersionOrder : IComparer<MinecraftVersionInfo>
    {
        public int Compare(MinecraftVersionInfo? left, MinecraftVersionInfo? right)
        {
            if (left is null) return right is null ? 0 : -1; if (right is null) return 1;
            if (left.Kind == MinecraftVersionKind.Release && right.Kind == MinecraftVersionKind.Release && Version.TryParse(left.Id, out var a) && Version.TryParse(right.Id, out var b))
                return a.CompareTo(b);
            return left.ReleasedAt.CompareTo(right.ReleasedAt);
        }
    }
}
