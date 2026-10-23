using System.IO.Compression;
using System.Diagnostics;
using System.Text.Json;
using SHLauncher.Contracts.Content;
using SHLauncher.Contracts.Engine;
using SHLauncher.Infrastructure.Content;
internal static class ModScenarios
{
    public static async Task Run()
    {
        var root=new DirectoryInfo(AppContext.BaseDirectory);while(root is not null&&!File.Exists(Path.Combine(root.FullName,"SHLauncherNext.sln")))root=root.Parent;
        var folder=Path.Combine(root!.FullName,".devdata","mod-tests",Guid.NewGuid().ToString("N"));var mods=Path.Combine(folder,"game","mods");Directory.CreateDirectory(mods);
        var instance=new InstanceInfo("fixture","Fixture","1.20.1","Fabric",17,4096,"","","",new(folder,folder,Path.Combine(folder,"game")));
        void Check(bool value,string name){if(!value)throw new Exception(name);}
        void Jar(string path,string entry,string text){using var zip=ZipFile.Open(path,ZipArchiveMode.Create);using var writer=new StreamWriter(zip.CreateEntry(entry).Open());writer.Write(text);}
        var gate=new SemaphoreSlim(1,1);var recycle=new FixtureRecycle(Path.Combine(folder,"recycled"));var service=new InstanceModService(recycle,new NoOpen(),_=>gate,()=>true,_=>true);
        var readOnly=new InstanceModService(recycle,new NoOpen(),_=>gate,()=>true,_=>false);
        var absent=instance with {Location=new(folder,folder,Path.Combine(folder,"readonly"))};
        using(var ignored=readOnly.Watch(absent,()=>{}))Check(!Directory.Exists(Path.Combine(folder,"readonly")),"readonly watch never creates directories");
        try{await readOnly.InstallLocalAsync(absent,[],default);throw new Exception("readonly write accepted");}catch(IOException){}
        Check(!Directory.Exists(Path.Combine(folder,"readonly")),"readonly write denied before directory creation");
        var busyService=new InstanceModService(recycle,new NoOpen(),_=>gate,()=>false,_=>true);
        try{await busyService.SetEnabledAsync(instance,[],false,default);throw new Exception("running game mutation accepted");}catch(IOException){}
        await gate.WaitAsync();try{try{await service.SetEnabledAsync(instance,[],false,default);throw new Exception("locked instance mutation accepted");}catch(IOException){}}finally{gate.Release();}
        var timings=new List<object>();var previous=0;
        foreach(var count in new[]{100,300,600})
        {
            for(var i=previous;i<count;i++)Jar(Path.Combine(mods,$"mod-{i}.jar"),"fabric.mod.json",JsonSerializer.Serialize(new{id="mod_"+i,version="1.0",name="Mod "+i,authors=new[]{"Author"},depends=new{minecraft="1.20.1"}}));
            var clock=Stopwatch.StartNew();var result=await service.ScanAsync(instance,true,default);Check(result.Count==count&&result.All(m=>m.Loader=="Fabric"&&m.Dependencies.Count==1),"metadata scan "+count);timings.Add(new{Count=count,Milliseconds=clock.Elapsed.TotalMilliseconds});previous=count;
        }
        var disabled=await service.SetEnabledAsync(instance,["mod-0.jar"],false,default);Check(disabled.Single().Succeeded&&File.Exists(Path.Combine(mods,"mod-0.jar.disabled")),"disable physically renames file");
        File.Copy(Path.Combine(mods,"mod-1.jar"),Path.Combine(mods,"mod-0.jar"));var collision=await service.SetEnabledAsync(instance,["mod-0.jar.disabled"],true,default);Check(!collision.Single().Succeeded&&File.Exists(Path.Combine(mods,"mod-0.jar.disabled")),"collision preserves both files");
        var bad=await service.SetEnabledAsync(instance,["../outside.jar"],false,default);Check(!bad.Single().Succeeded,"path escape rejected");
        var source=Path.Combine(folder,"local.jar");Jar(source,"quilt.mod.json","""{"quilt_loader":{"id":"quilt_test","version":"1","metadata":{"name":"Quilt Test"}}}""");
        var installed=await service.InstallLocalAsync(instance,[source],default);Check(installed.Single().Succeeded&&File.Exists(source)&&File.Exists(Path.Combine(mods,"local.jar")),"local install preserves original and publishes validated copy");
        var changed=new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);using(var watch=service.Watch(instance,()=>changed.TrySetResult())){File.WriteAllText(Path.Combine(mods,"bad.jar"),"broken");await changed.Task.WaitAsync(TimeSpan.FromSeconds(5));}
        Check((await service.ScanAsync(instance,true,default)).Any(m=>m.FileName=="bad.jar"&&m.Problem is not null),"bad jar isolated and watcher notifies");
        var recycled=await service.RecycleAsync(instance,["bad.jar"],default);Check(recycled.Single().Succeeded&&File.Exists(Path.Combine(folder,"recycled","bad.jar")),"deletion delegates to recycle service, not permanent delete");
        foreach(var pair in new[]{("forge.jar","META-INF/mods.toml"),("neo.jar","META-INF/neoforge.mods.toml")})
        {
            var file=Path.Combine(mods,pair.Item1);Jar(file,pair.Item2,"modLoader=\"javafml\"\n[[mods]]\nmodId=\"sample\"\nversion=\"1.2\"\ndisplayName=\"Sample\"\n[[dependencies.sample]]\nmodId=\"minecraft\"\nmandatory=true\nversionRange=\"[1.20.1]\"\n");
            var parsed=ModMetadataReader.Read(file);Check(parsed.Id=="sample"&&parsed.Dependencies.Count==1,"real TOML parser "+pair.Item1);
        }
        await File.WriteAllTextAsync(Path.Combine(folder,"timings.json"),JsonSerializer.Serialize(timings));Console.WriteLine("PASS: Mods 100/300/600 metadata, TOML, rename/collision, local copy, path boundaries, malformed jars, watcher, recycle delegation. "+JsonSerializer.Serialize(timings));
    }
    private sealed class NoOpen:IPathOpener{public void OpenFolder(string directory){}}
    private sealed class FixtureRecycle(string folder):IRecycleService{public Task RecycleAsync(string path,CancellationToken token){Directory.CreateDirectory(folder);File.Move(path,Path.Combine(folder,Path.GetFileName(path)));return Task.CompletedTask;}}
}
