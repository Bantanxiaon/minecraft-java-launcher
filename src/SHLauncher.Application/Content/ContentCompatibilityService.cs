using SHLauncher.Contracts.Content;
namespace SHLauncher.Application.Content;
public static class ContentCompatibilityService
{
    public static ContentCompatibility Evaluate(ContentVersion version, string minecraft, string loader,
        IReadOnlySet<string> presentContentIds)
    {
        if (version.MinecraftVersions.Count == 0 || version.Loaders.Count == 0)
            return new(CompatibilityStatus.UnknownCompatibility, "兼容性信息不足。");
        if (!version.MinecraftVersions.Contains(minecraft)) return new(CompatibilityStatus.IncompatibleMinecraftVersion, "Minecraft 版本不匹配。");
        if (!version.Loaders.Contains(loader)) return new(CompatibilityStatus.WrongLoader, "Loader 不匹配。");
        if (version.Dependencies.Any(d => d.Kind == DependencyKind.Required && !presentContentIds.Contains(d.ProviderId + ":" + d.ContentId)))
            return new(CompatibilityStatus.MissingDependency, "缺少必需的依赖内容。");
        if (version.Dependencies.Any(d => d.Kind is DependencyKind.Required or DependencyKind.Incompatible or DependencyKind.Unknown))
            return new(CompatibilityStatus.UnknownCompatibility, "已有项目 ID 不足以证明依赖版本或冲突约束可满足，需要完整图解析。");
        return new(CompatibilityStatus.Compatible, "声明的版本、Loader 和依赖相符；尚未验证内容本身。");
    }
}
