using System.Text.Json;
using SHLauncher.Contracts.Install;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Infrastructure.Install;
internal static class DiskTaskStoreScenarios
{
    internal static async Task Run()
    {
        var root = new DirectoryInfo(AppContext.BaseDirectory);
        while (root is not null && !File.Exists(Path.Combine(root.FullName, "SHLauncherNext.sln"))) root = root.Parent;
        var folder = Path.Combine(root!.FullName, ".devdata", "task-store-tests", Guid.NewGuid().ToString("N"));
        var recipe = new InstallationRecipe(Guid.NewGuid(), "26.3", LoaderType.Vanilla, "", "valid");
        var record = new InstallationTaskRecord(recipe, new(recipe.Id, "Minecraft", DownloadTaskState.Downloading, new(1, 2), null));
        var store = new JsonInstallationTaskStore(folder); await store.SaveAsync(record, default);
        if ((await new JsonInstallationTaskStore(folder).LoadAsync(default)).Single() != record) throw new Exception("Disk record roundtrip.");
        var file = Path.Combine(folder, recipe.Id.ToString("N") + ".json");
        foreach (var bad in new[] { "null", "{\"Recipe\":null,\"Snapshot\":null}",
            JsonSerializer.Serialize(record with { Snapshot = record.Snapshot with { State = (DownloadTaskState)999 } }),
            JsonSerializer.Serialize(record with { Recipe = recipe with { DisplayName = new string('x',129) } }),
            JsonSerializer.Serialize(record with { Recipe = recipe with { Id = Guid.NewGuid() } }) })
        {
            await File.WriteAllTextAsync(file, bad); var rejected = false;
            try { await store.LoadAsync(default); } catch (InstallationPersistenceException) { rejected = true; }
            if (!rejected || await File.ReadAllTextAsync(file) != bad) throw new Exception("Corrupt task must be rejected and preserved.");
        }
        var preferencesPath = Path.Combine(folder,"preferences","downloads.json");
        var preferences = new SHLauncher.Infrastructure.Runtime.DownloadSourcePreferences(preferencesPath);
        await preferences.SetModeAsync(DownloadSourceMode.MirrorPreferred, default);
        if (new SHLauncher.Infrastructure.Runtime.DownloadSourcePreferences(preferencesPath).Mode != DownloadSourceMode.MirrorPreferred)
            throw new Exception("Global download policy must survive restart.");
        Console.WriteLine("PASS: own global source preferences persist across instances.");
        Console.WriteLine("PASS: real disk roundtrip; null, enum, traversal and identity corruption preserved and rejected.");
    }
}
