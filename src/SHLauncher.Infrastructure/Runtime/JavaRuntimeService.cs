using System.Diagnostics;
using System.Text;
using SHLauncher.Contracts.Runtime;
namespace SHLauncher.Infrastructure.Runtime;
public sealed class JavaRuntimeService : IJavaRuntimeService
{
    public async Task<IReadOnlyList<JavaRuntime>> DetectAsync(IEnumerable<string> candidates, CancellationToken token)
    {
        var paths = new HashSet<string>(OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal);
        foreach (var path in candidates) Add(path);
        var home = Environment.GetEnvironmentVariable("JAVA_HOME");
        if (!string.IsNullOrWhiteSpace(home)) Add(Path.Combine(home, "bin", OperatingSystem.IsWindows() ? "java.exe" : "java"));
        foreach (var folder in (Environment.GetEnvironmentVariable("PATH") ?? "").Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries))
            Add(Path.Combine(folder.Trim('"'), OperatingSystem.IsWindows() ? "java.exe" : "java"));
        var results = new List<JavaRuntime>();
        foreach (var path in paths.Take(128))
        {
            token.ThrowIfCancellationRequested();
            if (File.Exists(path)) results.Add(await ProbeAsync(path, token).ConfigureAwait(false));
        }
        return results.AsReadOnly();
        void Add(string value) { if (Path.IsPathFullyQualified(value)) paths.Add(Path.GetFullPath(value)); }
    }
    public async Task<JavaRuntime> ProbeAsync(string executable, CancellationToken token)
    {
        if (!Path.IsPathFullyQualified(executable)) return Invalid("需要绝对 Java 路径。");
        executable = Path.GetFullPath(executable);
        if (!File.Exists(executable)) return Invalid("Java 文件不存在。");
        // javaw cannot reliably return console properties. Probe its sibling, retain a console launch path.
        if (OperatingSystem.IsWindows() && Path.GetFileName(executable).Equals("javaw.exe", StringComparison.OrdinalIgnoreCase))
            executable = Path.Combine(Path.GetDirectoryName(executable)!, "java.exe");
        using var process = new Process { StartInfo = new(executable) { UseShellExecute = false, CreateNoWindow = true,
            RedirectStandardOutput = true, RedirectStandardError = true } };
        process.StartInfo.ArgumentList.Add("-XshowSettings:properties"); process.StartInfo.ArgumentList.Add("-version");
        // Ignore inherited injection options during probing, without changing the system environment.
        foreach (var key in new[] { "JAVA_TOOL_OPTIONS", "_JAVA_OPTIONS", "JDK_JAVA_OPTIONS", "SH_CURSEFORGE_API_KEY", "SH_CURSEFORGE_CONFIG_FILE" }) process.StartInfo.Environment.Remove(key);
        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(token); timeout.CancelAfter(TimeSpan.FromSeconds(10));
        Task<string>? output = null, error = null;
        try
        {
            process.Start();
            output = ReadBounded(process.StandardOutput, timeout.Token); error = ReadBounded(process.StandardError, timeout.Token);
            await process.WaitForExitAsync(timeout.Token).ConfigureAwait(false);
            var text = await output.ConfigureAwait(false) + "\n" + await error.ConfigureAwait(false);
            var properties = text.Split('\n').Select(l => l.Trim().Split(" = ", 2)).Where(p => p.Length == 2)
                .GroupBy(p => p[0]).ToDictionary(g => g.Key, g => g.Last()[1].Trim());
            var version = properties.GetValueOrDefault("java.version", "");
            var part = version.StartsWith("1.", StringComparison.Ordinal) ? version.Split('.').ElementAtOrDefault(1) : version.Split('.')[0];
            var major = int.TryParse(part, out var n) ? n : 0;
            var arch = properties.GetValueOrDefault("os.arch", "") switch { "amd64" or "x86_64" => "x64", "aarch64" or "arm64" => "arm64", "x86" or "i386" => "x86", _ => "unknown" };
            return new(executable, version, major, arch, properties.GetValueOrDefault("java.vendor"), process.ExitCode == 0 && major > 0 && arch != "unknown");
        }
        catch (OperationCanceledException) when (!token.IsCancellationRequested) { return Invalid("Java 检测超时。"); }
        catch (Exception e) when (e is System.ComponentModel.Win32Exception or IOException or InvalidOperationException) { return Invalid(e.GetType().Name); }
        finally
        {
            try { if (process.Id > 0 && !process.HasExited) { process.Kill(true); await process.WaitForExitAsync().ConfigureAwait(false); } } catch (InvalidOperationException) { }
            foreach (var task in new[] { output, error }) if (task is not null) try { await task.ConfigureAwait(false); } catch (OperationCanceledException) { }
        }
        JavaRuntime Invalid(string message) => new(executable, "", 0, "unknown", null, false, message);
    }
    private static async Task<string> ReadBounded(StreamReader reader, CancellationToken token)
    {
        var text = new StringBuilder(); var buffer = new char[2048]; int read;
        while ((read = await reader.ReadAsync(buffer.AsMemory(), token).ConfigureAwait(false)) != 0)
            if (text.Length < 65536) text.Append(buffer, 0, Math.Min(read, 65536 - text.Length));
        return text.ToString();
    }
}
