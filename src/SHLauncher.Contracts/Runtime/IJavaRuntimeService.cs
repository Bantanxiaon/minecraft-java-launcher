namespace SHLauncher.Contracts.Runtime;
public sealed record JavaRuntime(string ExecutablePath, string Version, int Major, string Architecture,
    string? Vendor, bool IsUsable, string? Problem = null);
public interface IJavaRuntimeService
{
    Task<IReadOnlyList<JavaRuntime>> DetectAsync(IEnumerable<string> candidates, CancellationToken token);
    Task<JavaRuntime> ProbeAsync(string executable, CancellationToken token);
}
public enum JavaSelectionIssue { Missing, Mismatch, UnknownRequirement }
public sealed record JavaSelectionResult(JavaRuntime? Runtime, string? Problem, JavaSelectionIssue? Issue = null);
public interface IJavaSelectionService
{
    JavaSelectionResult Select(int requiredMajor, string architecture, string? manualPath, IReadOnlyList<JavaRuntime> runtimes);
}
