using System.Security.Cryptography;
using System.Text.Json;
using System.Text.Json.Nodes;
using CmlLib.Core.Installer.Forge;
namespace SHLauncher.Engine.CmlLib;
internal static class ForgeProcessorPipeline
{
    internal static async Task RunAsync(string java, string staging, string clientJar, string libraries, string root, JsonElement profile, Action<string> output, CancellationToken token)
    {
        var processor = new ForgeInstallProcessor(java);
        var mapping = processor.MapProcessorData(profile.GetProperty("data"), clientJar, libraries, staging);
        var clientProcessors = profile.GetProperty("processors").EnumerateArray().Where(p =>
            !p.TryGetProperty("sides", out var sides) || sides.EnumerateArray().Any(s => s.GetString() == "client")).ToArray();
        foreach (var item in clientProcessors)
        {
            var jar = PathSafety.EnsureContained(root, Path.Combine(libraries, ForgePackageName.GetPath(item.GetProperty("jar").GetString()!, Path.DirectorySeparatorChar)));
            if (!File.Exists(jar) || !HasMainClass(jar)) throw new InvalidDataException("Processor has no validated executable entry.");
            foreach (var name in item.GetProperty("classpath").EnumerateArray())
                if (!File.Exists(PathSafety.EnsureContained(root, Path.Combine(libraries, ForgePackageName.GetPath(name.GetString()!, Path.DirectorySeparatorChar))))) throw new InvalidDataException("Processor classpath missing.");
            // Preselect client processors; normalize only the already selected side marker for the mature runner.
            var node = JsonNode.Parse(item.GetRawText())!; node["sides"] = new JsonArray("client");
            using var single = JsonDocument.Parse("[" + node.ToJsonString() + "]");
            await processor.StartProcessors(single.RootElement, mapping, libraries, null, new Forward(output), token).ConfigureAwait(false);
            if (item.TryGetProperty("outputs", out var expected))
                foreach (var entry in expected.EnumerateObject())
                {
                    var path = PathSafety.EnsureContained(root, ForgeMapper.Interpolation(entry.Name, mapping, true).Trim('"'));
                    var sha1 = ForgeMapper.Interpolation(entry.Value.GetString()!, mapping, true).Trim('"');
                    if (sha1.Length != 40 || sha1.Any(c => !Uri.IsHexDigit(c)) || !File.Exists(path)) throw new InvalidDataException("Processor expected output unavailable.");
                    using var input = File.OpenRead(path);
                    if (!Convert.ToHexString(await SHA1.HashDataAsync(input, token).ConfigureAwait(false)).Equals(sha1, StringComparison.OrdinalIgnoreCase)) throw new InvalidDataException("Processor output hash mismatch.");
                }
        }
    }
    private static bool HasMainClass(string jar)
    {
        using var zip = new ICSharpCode.SharpZipLib.Zip.ZipFile(jar);
        var entry = zip.GetEntry("META-INF/MANIFEST.MF");
        if (entry is null || entry.Size is < 0 or > 65536) return false;
        using var input = zip.GetInputStream(entry); using var reader = new StreamReader(input);
        return reader.ReadToEnd().Split('\n').Any(line => line.StartsWith("Main-Class:", StringComparison.OrdinalIgnoreCase) && line[11..].Trim().Length > 0);
    }
    private sealed class Forward(Action<string> action) : IProgress<string>
    { public void Report(string value) => action(value.Length > 2048 ? value[..2048] : value); }
}
