using System.Diagnostics;
using System.Security.Cryptography;
using System.Text.Json;
using SHLauncher.Contracts.Downloads;
internal static class CrossProcessScenarios
{
    internal static async Task Run(LocalHttpServer server, string work)
    {
        var root = Path.Combine(work, "process-restart"); Directory.CreateDirectory(root);
        Process Start(string endpoint, byte[] expected)
        {
            var executable = Environment.ProcessPath ?? throw new Exception("Test process unavailable.");
            var info = new ProcessStartInfo(executable) { UseShellExecute = false, RedirectStandardOutput = true, RedirectStandardError = true, CreateNoWindow = true };
            if (Path.GetFileNameWithoutExtension(executable).Equals("dotnet", StringComparison.OrdinalIgnoreCase)) info.ArgumentList.Add(System.Reflection.Assembly.GetExecutingAssembly().Location);
            foreach (var argument in new[] { "child-download", new Uri(server.BaseUri, endpoint).AbsoluteUri, root,
                Convert.ToHexString(SHA1.HashData(expected)), Convert.ToHexString(SHA256.HashData(expected)), expected.Length.ToString() }) info.ArgumentList.Add(argument);
            return Process.Start(info) ?? throw new Exception("Child process did not start.");
        }
        using (var interrupted = Start("slow", server.Payload))
        {
            var clock = Stopwatch.StartNew();
            try
            {
                while (!Directory.GetFiles(root, "*.part").Any(p => new FileInfo(p).Length > 0))
                { if (interrupted.HasExited || clock.Elapsed > TimeSpan.FromSeconds(10)) throw new Exception("Child never produced partial bytes."); await Task.Delay(5); }
                interrupted.Kill(true); await interrupted.WaitForExitAsync();
            }
            finally { if (!interrupted.HasExited) { interrupted.Kill(true); await interrupted.WaitForExitAsync(); } }
        }
        if (File.Exists(Path.Combine(root, "target.bin"))) throw new Exception("Interrupted process left a fake complete file.");
        var parts = Directory.GetFiles(root, "*.part").ToDictionary(p => p, p => new FileInfo(p).Length);
        await Task.Delay(100);
        if (parts.Count == 0 || parts.Any(p => new FileInfo(p.Key).Length != p.Value)) throw new Exception("Orphan child continues writing after exit.");
        var changed = server.Payload.Select(b => (byte)(b ^ 1)).ToArray();
        using (var restarted = Start("corrupt", changed))
        {
            var stderr = restarted.StandardError.ReadToEndAsync(); var stdout = restarted.StandardOutput.ReadToEndAsync();
            using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(10));
            try { await restarted.WaitForExitAsync(deadline.Token); }
            finally { if (!restarted.HasExited) { restarted.Kill(true); await restarted.WaitForExitAsync(); } }
            if (restarted.ExitCode != 0) throw new Exception("Restart process failed: " + await stderr + await stdout);
        }
        var result = JsonSerializer.Deserialize<DownloadResult>(await File.ReadAllTextAsync(Path.Combine(root, "result.json")))!;
        var actual = await File.ReadAllBytesAsync(Path.Combine(root, "target.bin"));
        if (!result.Succeeded || !changed.SequenceEqual(actual) || parts.Keys.Any(p => !File.Exists(p)))
            throw new Exception("Changed task metadata reused stale partial state or lost diagnostic evidence.");
        await File.WriteAllTextAsync(Path.Combine(root, "interruption-evidence.json"), JsonSerializer.Serialize(new { InterruptedParts = parts, Restart = result, ChangedSourceAndExpectedHash = true, OrphanWrites = false }));
    }
}
