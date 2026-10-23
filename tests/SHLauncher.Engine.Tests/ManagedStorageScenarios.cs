using System.Text.Json;
using System.Security.Cryptography;
using SHLauncher.Bootstrap;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Install;
using SHLauncher.Core.Launching;
internal static class ManagedStorageScenarios
{
    internal static async Task Run(string workspace,string java)
    {
        await using var owner=new LauncherComposition(workspace);owner.ManualJava=java;
        await owner.Coordinator.InitializeAsync();await owner.Installations.InitializeAsync(default);
        var checks=new List<string>();
        var instancesRoot=Path.Combine(workspace,"instances");
        var existingInstances=Directory.Exists(instancesRoot)?Directory.EnumerateDirectories(instancesRoot).Count(p=>File.Exists(Path.Combine(p,"instance.json"))):0;
        void Check(bool condition,string message){if(!condition)throw new Exception(message);checks.Add(message);Console.WriteLine("PASS "+message);}
        async Task Until(Func<bool> predicate,int seconds=120)
        {using var stop=new CancellationTokenSource(TimeSpan.FromSeconds(seconds));while(!predicate())await Task.Delay(20,stop.Token);}
        async Task<InstallResult> Install(InstallationRecipe recipe)
        {
            Check(owner.Installations.Enqueue(recipe),"managed task accepted "+recipe.InstanceKey);
            await Until(()=>owner.Installations.Records.Any(r=>r.Recipe.Id==recipe.Id&&r.Snapshot.State is DownloadTaskState.Completed or DownloadTaskState.Failed));
            await owner.Installations.CancelAsync(recipe.Id);
            var record=owner.Installations.Records.Single(r=>r.Recipe.Id==recipe.Id);
            if(record.Result is null)throw new Exception("Managed install failed: "+string.Join("\n",owner.GameLogs.Snapshot(recipe.Id).Select(l=>l.Message)));
            return record.Result;
        }
        var first=new InstallationRecipe(Guid.NewGuid(),"26.3",LoaderType.Vanilla,"","同名显示测试",Guid.NewGuid());
        var second=first with{Id=Guid.NewGuid(),InstanceKey=Guid.NewGuid()};
        var one=await Install(first);var two=await Install(second);
        var firstFolder=Path.GetDirectoryName(one.Root)!;var secondFolder=Path.GetDirectoryName(two.Root)!;
        Check(Path.GetFileName(firstFolder)==first.InstanceKey.ToString("N")&&Path.GetFileName(secondFolder)==second.InstanceKey.ToString("N")&&firstFolder!=secondFolder,"friendly names do not determine physical identity");
        Check(File.Exists(Path.Combine(firstFolder,"instance.json"))&&Directory.Exists(Path.Combine(firstFolder,"game"))&&!Directory.Exists(Path.Combine(firstFolder,first.DisplayName)),"published instance has descriptor, game and runtime");
        var config=Path.Combine(firstFolder,"game","config","storage-test.json");Directory.CreateDirectory(Path.GetDirectoryName(config)!);await File.WriteAllTextAsync(config,"preserve own config");
        Check(!File.Exists(Path.Combine(secondFolder,"game","config","storage-test.json")),"instance-local config stays isolated");
        var runtimeBefore=await Hash(one.Root);
        var warm=await Install(first with{Id=Guid.NewGuid()});
        Check(warm.NetworkBytes==0&&await File.ReadAllTextAsync(config)=="preserve own config"&&runtimeBefore==await Hash(one.Root),"warm reinstall reuses artifact cache and preserves game/config and runtime hashes");
        await owner.OpenInstalledAsync(one,default);
        Check(owner.Coordinator.Instances.Count==existingInstances+2&&owner.Coordinator.Instances.All(i=>Guid.TryParseExact(i.Id,"N",out _))&&owner.Coordinator.Selected?.Name==first.DisplayName,"managed library shows friendly names with stable GUID identities");
        await owner.RenameInstanceAsync(first.InstanceKey,"改名后的原版",default);
        Check(owner.Coordinator.Selected?.Name=="改名后的原版"&&Directory.Exists(firstFolder)&&runtimeBefore==await Hash(one.Root),"rename changes descriptor without moving or modifying runtime artifacts");
        var launch=owner.Coordinator.StartAsync();await Until(()=>owner.Coordinator.Snapshot.State is LauncherTaskState.Running or LauncherTaskState.Failed,45);
        Check(owner.Coordinator.Snapshot.State==LauncherTaskState.Running,"managed GUID dispatch starts real game profile");
        owner.Coordinator.CancelOrStop();await launch.WaitAsync(TimeSpan.FromSeconds(30));
        Check(owner.Coordinator.Snapshot.State==LauncherTaskState.Idle,"managed process Stop drains before ownership release");
        await owner.Installations.ShutdownAsync();
        await File.WriteAllTextAsync(Path.Combine(workspace,"managed-storage-result.json"),JsonSerializer.Serialize(new{Passed=true,Checks=checks,First=first.InstanceKey,Second=second.InstanceKey,FirstPath=firstFolder,SecondPath=secondFolder,SharedCache=Path.Combine(workspace,"shared","cache"),Note="Metadata HTTP refresh excluded from artifact NetworkBytes. Real process start/Stop is not main-menu proof."}));
    }
    private static async Task<string> Hash(string root)
    {
        using var hash=IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        foreach(var file in Directory.EnumerateFiles(root,"*",SearchOption.AllDirectories).Where(p=>!p.EndsWith(".part",StringComparison.Ordinal)).Order(StringComparer.Ordinal))
        {using var stream=File.OpenRead(file);hash.AppendData(await SHA256.HashDataAsync(stream));}
        return Convert.ToHexString(hash.GetHashAndReset());
    }
}
