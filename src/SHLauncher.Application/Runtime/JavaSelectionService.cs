using SHLauncher.Contracts.Runtime;
namespace SHLauncher.Application.Runtime;
public sealed class JavaSelectionService : IJavaSelectionService
{
    public JavaSelectionResult Select(int requiredMajor, string architecture, string? manualPath, IReadOnlyList<JavaRuntime> runtimes)
    {
        if (requiredMajor <= 0) return new(null, "无法可靠确定该版本需要的 Java。", JavaSelectionIssue.UnknownRequirement);
        if (OperatingSystem.IsWindows() && manualPath is not null && Path.GetFileName(manualPath).Equals("javaw.exe", StringComparison.OrdinalIgnoreCase))
            manualPath = Path.Combine(Path.GetDirectoryName(manualPath) ?? "", "java.exe");
        if (manualPath is not null && Path.IsPathFullyQualified(manualPath)) manualPath = Path.GetFullPath(manualPath);
        var comparison = OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal;
        var candidates = string.IsNullOrWhiteSpace(manualPath) ? runtimes : runtimes.Where(x => string.Equals(x.ExecutablePath, manualPath, comparison));
        var runtime = candidates.Where(x => x.IsUsable && x.Major == requiredMajor && x.Architecture == architecture)
            .OrderByDescending(x => Version.TryParse(x.Version, out var parsed) ? parsed : new Version(0, 0)).FirstOrDefault();
        return runtime is null ? new(null, string.IsNullOrWhiteSpace(manualPath)
            ? $"需要可用的 {architecture} Java {requiredMajor}，请指定现有 Java。"
            : $"指定的 Java 不可用，或与 Java {requiredMajor} / {architecture} 要求不符。",
                candidates.Any(x => x.IsUsable) ? JavaSelectionIssue.Mismatch : JavaSelectionIssue.Missing) : new(runtime, null);
    }
}
