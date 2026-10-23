using SHLauncher.Contracts.Content;
namespace SHLauncher.Application.Versions;
public sealed class CompatibilityService : ICompatibilityService
{
    public CompatibilityResult Check(ResolvedContentVersion version, InstanceCompatibilityContext context)
    {
        if (version.MinecraftVersions.Count == 0 || version.Loaders.Count == 0 ||
            string.IsNullOrWhiteSpace(context.MinecraftVersion) || string.IsNullOrWhiteSpace(context.Loader))
            return new(CompatibilityCode.Unknown, "缺少可靠的版本或 Loader 数据。", []);
        if (!version.MinecraftVersions.Contains(context.MinecraftVersion, StringComparer.Ordinal))
            return new(CompatibilityCode.WrongMinecraftVersion, "不支持当前 Minecraft 版本。", [version.Project.ProjectId]);
        if (!version.Loaders.Contains(context.Loader, StringComparer.OrdinalIgnoreCase))
            return new(CompatibilityCode.WrongLoader, "不支持当前 Loader。", [version.Project.ProjectId]);
        return new(CompatibilityCode.Compatible, "Minecraft 与 Loader 精确匹配；依赖图仍须单独解析。", []);
    }
}
