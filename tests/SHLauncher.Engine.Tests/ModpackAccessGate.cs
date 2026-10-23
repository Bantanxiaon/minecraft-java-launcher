using System.Text.Json;
using SHLauncher.Bootstrap;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Install;
internal static class ModpackAccessGate
{
    internal static async Task Run(string dataRoot,string archive,string report)
    {
        if(Directory.Exists(dataRoot))throw new InvalidOperationException("Use a new isolated data root for this gate.");
        var checks=new List<string>();
        void Check(bool value,string name){if(!value)throw new Exception(name);checks.Add(name);}
        ModpackPreview preview;InstallationRecipe recipe;
        await using(var owner=new LauncherComposition(dataRoot))
        {
            await owner.Installations.InitializeAsync(default);
            preview=await owner.ModpackInspector.InspectAsync(archive,default);
            recipe=new(Guid.NewGuid(),preview.MinecraftVersion,preview.Loader,preview.LoaderVersion,"真实包访问边界验证",Guid.NewGuid(),archive,preview.ArchiveSha256);
            Check(owner.Installations.Enqueue(recipe),"real archive accepted by production queue");
            Check(!owner.Installations.Enqueue(recipe with{Id=Guid.NewGuid(),InstanceKey=Guid.NewGuid()}),"same active archive rejected before duplicate worker");
            await Wait(owner,recipe.Id);
            var record=owner.Installations.Records.Single(r=>r.Recipe.Id==recipe.Id);
            Check(record.Snapshot.State==DownloadTaskState.Failed&&record.Result is null&&record.Snapshot.Problem?.Contains("CurseForge",StringComparison.Ordinal)==true,"missing access is a visible failed task, never success");
            Check(owner.GameLogs.Snapshot(recipe.Id).Any(e=>e.Message.Contains("ProviderAccessMissing",StringComparison.Ordinal)),"structured missing-access diagnostic");
            Check(owner.DownloadEvents(recipe.Id).Count==0,"no metadata or binary download attempted");
            Check(!Directory.Exists(Path.Combine(dataRoot,"instances"))&&!Directory.Exists(Path.Combine(dataRoot,"temp","install")),"no instance or install staging created");
            Check(owner.Installations.Retry(recipe.Id),"blocked task can be explicitly retried");
            var retried=owner.Installations.Records.First(r=>r.Recipe.Id!=recipe.Id);
            Check(retried.Recipe.InstanceKey==recipe.InstanceKey,"retry retains instance GUID");
            await Wait(owner,retried.Recipe.Id);
            Check(owner.Installations.Records.Single(r=>r.Recipe.Id==retried.Recipe.Id).Snapshot.State==DownloadTaskState.Failed,"retry preserves truthful external blocker");
        }
        await using(var reopened=new LauncherComposition(dataRoot))
        {
            await reopened.Installations.InitializeAsync(default);
            Check(reopened.Installations.Records.Count==2&&reopened.Installations.Records.All(r=>r.Snapshot.State==DownloadTaskState.Failed&&r.Result is null),"failed tasks persist across restart");
            Check(!Directory.Exists(Path.Combine(dataRoot,"instances")),"restart never publishes a failed pack");
        }
        await File.WriteAllTextAsync(report,JsonSerializer.Serialize(new{Passed=true,Scope="Real ZIP and production queue; missing access only. Not full install or launch proof.",preview.ArchiveSha256,preview.ArchiveSize,preview.MinecraftVersion,preview.Loader,preview.LoaderVersion,RemoteFiles=preview.RemoteFiles.Count,LocalFiles=preview.LocalFiles.Count,Checks=checks,PhysicalDrag="NOT VERIFIED",RealInstall="BLOCKED: SH Launcher-owned CurseForge access unavailable"},new JsonSerializerOptions{WriteIndented=true}));
        Console.WriteLine($"PASS: {checks.Count} real-ZIP production access-boundary checks; no remote requests, no published instance. Full pack gate BLOCKED.");
    }
    private static async Task Wait(LauncherComposition owner,Guid id)
    {
        using var timeout=new CancellationTokenSource(TimeSpan.FromSeconds(60));
        while(owner.Installations.Records.Single(r=>r.Recipe.Id==id).Snapshot.State is not (DownloadTaskState.Completed or DownloadTaskState.Failed or DownloadTaskState.Cancelled))
            await Task.Delay(50,timeout.Token);
        await owner.Installations.CancelAsync(id); // Terminal already observed: await durable write, do not cancel active work.
    }
}
