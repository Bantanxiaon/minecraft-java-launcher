using ICSharpCode.SharpZipLib.Zip;
namespace SHLauncher.Engine.CmlLib;
internal static class InstallerArchive
{
    internal static async Task CopyMavenAsync(string extracted, string libraries, CancellationToken token)
    {
        var maven = Path.Combine(extracted, "maven");
        if (!Directory.Exists(maven)) return;
        foreach (var source in Directory.EnumerateFiles(maven, "*", SearchOption.AllDirectories))
        {
            token.ThrowIfCancellationRequested();
            PathSafety.EnsureContained(extracted, source);
            var destination = PathSafety.EnsureContained(libraries, Path.Combine(libraries, Path.GetRelativePath(maven, source)));
            Directory.CreateDirectory(Path.GetDirectoryName(destination)!);
            var part = destination + "." + Guid.NewGuid().ToString("N") + ".part";
            await using (var input = File.OpenRead(source))
            await using (var output = new FileStream(part, FileMode.CreateNew, FileAccess.Write, FileShare.None, 81920, true))
            { await input.CopyToAsync(output, token).ConfigureAwait(false); await output.FlushAsync(token).ConfigureAwait(false); output.Flush(true); }
            using var original = File.OpenRead(source); using var copied = File.OpenRead(part);
            var expected = await System.Security.Cryptography.SHA256.HashDataAsync(original, token).ConfigureAwait(false);
            var actual = await System.Security.Cryptography.SHA256.HashDataAsync(copied, token).ConfigureAwait(false);
            if (!expected.SequenceEqual(actual)) throw new InvalidDataException("Embedded Maven copy integrity failed.");
            // Source bytes inherit trust from the verified installer archive; this is a copy check, not a remote expected hash.
            copied.Close(); token.ThrowIfCancellationRequested(); File.Move(part, destination, true);
        }
    }
    internal static async Task ExtractAsync(string archive, string destination, CancellationToken token)
    {
        if (Directory.Exists(destination)) throw new IOException("Installer extraction directory must be new.");
        using var zip = new ZipFile(archive);
        var entries = zip.Cast<ZipEntry>().ToArray();
        if (entries.Length > 20000) throw new InvalidDataException("Installer archive entry budget exceeded.");
        long total = 0;
        var names = new HashSet<string>(OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal);
        foreach (var entry in entries)
        {
            token.ThrowIfCancellationRequested();
            var target = PathSafety.EnsureContained(destination, Path.Combine(destination, entry.Name.Replace('\\', '/')));
            if (!names.Add(target)) throw new InvalidDataException("Duplicate archive entry.");
            if ((entry.ExternalFileAttributes&(int)FileAttributes.ReparsePoint)!=0 || ((entry.ExternalFileAttributes >> 16) & 0xF000) == 0xA000 || entry.IsCrypted || entry.Size < 0 || entry.Size > 256 * 1024 * 1024)
                throw new InvalidDataException("Unsupported installer entry.");
            total = checked(total + entry.Size);
            if (total > 512 * 1024 * 1024) throw new InvalidDataException("Installer extraction budget exceeded.");
        }
        Directory.CreateDirectory(destination);
        foreach (var entry in entries)
        {
            token.ThrowIfCancellationRequested();
            var target = PathSafety.EnsureContained(destination, Path.Combine(destination, entry.Name.Replace('\\', '/')));
            if (entry.IsDirectory) { Directory.CreateDirectory(target); continue; }
            Directory.CreateDirectory(Path.GetDirectoryName(target)!);
            using var input = zip.GetInputStream(entry);
            await using var output = new FileStream(target, FileMode.CreateNew, FileAccess.Write, FileShare.None, 81920, true);
            var buffer = new byte[81920]; long copied = 0; int read;
            while ((read = await input.ReadAsync(buffer, token).ConfigureAwait(false)) != 0)
            {
                copied += read; if (copied > entry.Size) throw new InvalidDataException("Archive entry exceeds declared size.");
                await output.WriteAsync(buffer.AsMemory(0, read), token).ConfigureAwait(false);
            }
            if (copied != entry.Size) throw new InvalidDataException("Truncated archive entry.");
        }
    }
}
