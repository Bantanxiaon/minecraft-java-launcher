using CmlLib.Core.Version;
using SHLauncher.Contracts.Engine;
using SHLauncher.Contracts.Instances;
namespace SHLauncher.Engine.CmlLib;
public sealed class CmlLibInstanceService : IInstanceService
{
    public Task<IReadOnlyList<InstanceInfo>> ScanAsync(string root, CancellationToken cancellationToken) =>
        Task.Run<IReadOnlyList<InstanceInfo>>(async () =>
        {
            if (!Path.IsPathFullyQualified(root) || !Directory.Exists(root))
                throw new LauncherEngineException(new(LauncherErrorCode.InstanceNotFound, "目录不存在", "请选择有效的 Minecraft 根目录。", null, false));
            root = Path.GetFullPath(root);
            var versions = Path.Combine(root, "versions");
            if (!Directory.Exists(versions)) return Array.Empty<InstanceInfo>();
            var results = new List<InstanceInfo>();
            foreach (var directory in Directory.EnumerateDirectories(versions).Order(StringComparer.Ordinal).Take(1000))
            {
                cancellationToken.ThrowIfCancellationRequested();
                var id = Path.GetFileName(directory);
                var location = new InstanceLocation(root, directory, directory);
                var info = new InstanceInfo(id, id, "未知", "Unknown", 0, 4096, "本地实例", "正在检查", "vanilla", location);
                IVersion? version = null;
                try
                {
                    PathSafety.EnsureContained(root, directory);
                    var metadata = Path.Combine(directory, id + ".json");
                    if (!File.Exists(metadata)) { results.Add(Invalid(info, LauncherErrorCode.MissingMetadata, "缺少版本元数据。")); continue; }
                    if (new FileInfo(metadata).Length > 8 * 1024 * 1024) { results.Add(Invalid(info, LauncherErrorCode.InstanceInvalid, "版本元数据过大。")); continue; }
                    version = await LocalVersionSource.ReadAsync(root, id, cancellationToken).ConfigureAwait(false);
                    // Only loader adapters that have reached their current gate are enabled; mature core resolves inheritance.
                    var names = version.ConcatInheritedCollection(v => v.Libraries).Select(l => l.Name).ToArray();
                    var loader = names.Any(n => n.StartsWith("net.neoforged:neoforge:", StringComparison.Ordinal)) ? "NeoForge" :
                        names.Any(n => n.StartsWith("net.minecraftforge:forge:", StringComparison.Ordinal)) ? "Forge" :
                        names.Any(n => n.StartsWith("org.quiltmc:quilt-loader:", StringComparison.Ordinal)) ? "Quilt" :
                        names.Any(n => n.StartsWith("net.fabricmc:fabric-loader:", StringComparison.Ordinal)) ? "Fabric" :
                        version.MainClass == "net.minecraft.client.main.Main" && version.InheritsFrom is null ? "Vanilla" : "Unknown";
                    var java = version.GetInheritedProperty(v => v.JavaVersion)?.MajorVersion;
                    info = info with { GameVersion = version.EnumerateToParent().Last().Id, Loader = loader, JavaMajor = int.TryParse(java, out var major) ? major : 0,
                        Description = loader is "Vanilla" or "Fabric" or "Forge" or "NeoForge" or "Quilt" ? "本地实例 · 启动前检查完整性" : "此 Loader 尚未通过当前阶段准入" };
                    if (loader is not ("Vanilla" or "Fabric" or "Forge" or "NeoForge" or "Quilt")) info = Invalid(info, LauncherErrorCode.UnsupportedVersion, "当前阶段尚不支持此实例的 Loader 或自定义版本。");
                    else if (loader == "Vanilla" && !File.Exists(Path.Combine(directory, id + ".jar"))) info = Invalid(info, LauncherErrorCode.GameFileMissing, "缺少游戏主文件。");
                    else if (info.JavaMajor == 0) info = Invalid(info, LauncherErrorCode.UnsupportedVersion, "该实例未提供可靠的 Java 要求。");
                    results.Add(info);
                }
                catch (OperationCanceledException) { throw; }
                catch (Exception e) when (e is IOException or InvalidDataException or FormatException or UnauthorizedAccessException or ArgumentException or System.Text.Json.JsonException or InvalidOperationException or KeyNotFoundException or VersionParseException or VersionDependencyException)
                { results.Add(Invalid(info, LauncherErrorCode.InstanceInvalid, "版本元数据无法读取或不完整。")); }
                finally { if (version is not null) LocalVersionSource.Dispose(version); }
            }
            return results.AsReadOnly();
        }, cancellationToken);
    private static InstanceInfo Invalid(InstanceInfo info, LauncherErrorCode code, string message) =>
        info with { IsLaunchable = false, Description = message, Problem = new(code, "该实例存在问题", message, null, false) };
}
