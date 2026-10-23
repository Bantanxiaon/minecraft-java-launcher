using ICSharpCode.SharpZipLib.Zip;
using SHLauncher.Engine.CmlLib;
internal static class ArchiveScenarios
{
    internal static async Task Run()
    {
        var directory = new DirectoryInfo(AppContext.BaseDirectory);
        while (directory is not null && !File.Exists(Path.Combine(directory.FullName, "SHLauncherNext.sln"))) directory = directory.Parent;
        var root = Path.Combine(directory!.FullName, ".devdata", "archive-tests", Guid.NewGuid().ToString("N")); Directory.CreateDirectory(root);
        var archive = Path.Combine(root, "bad.zip");
        using (var file = File.Create(archive)) using (var zip = new ZipOutputStream(file))
        { zip.PutNextEntry(new ZipEntry("../escape.txt")); zip.WriteByte(1); zip.CloseEntry(); }
        var rejected = false;
        try { await InstallerArchive.ExtractAsync(archive, Path.Combine(root, "extract"), default); } catch (InvalidDataException) { rejected = true; }
        if (!rejected || File.Exists(Path.Combine(root, "escape.txt")) || Directory.Exists(Path.Combine(root, "extract"))) throw new Exception("Installer archive traversal was not rejected before extraction.");
        var valid = Path.Combine(root, "good.zip");
        using (var file = File.Create(valid)) using (var zip = new ZipOutputStream(file))
        { zip.PutNextEntry(new ZipEntry("data/profile.json")); zip.WriteByte(123); zip.WriteByte(125); zip.CloseEntry(); }
        var target = Path.Combine(root, "valid"); await InstallerArchive.ExtractAsync(valid, target, default);
        if (await File.ReadAllTextAsync(Path.Combine(target, "data", "profile.json")) != "{}") throw new Exception("Valid archive extraction differs.");
        var cancel = new CancellationTokenSource(); cancel.Cancel(); var cancelled = false;
        try { await InstallerArchive.ExtractAsync(valid, Path.Combine(root, "cancel"), cancel.Token); } catch (OperationCanceledException) { cancelled = true; }
        if (!cancelled || Directory.Exists(Path.Combine(root, "cancel"))) throw new Exception("Cancelled archive wrote files.");
        Console.WriteLine("PASS: installer archive traversal, valid bytes and cancellation before write.");
    }
}
