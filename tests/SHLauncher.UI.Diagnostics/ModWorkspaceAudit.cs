using System.IO.Compression;
using System.Diagnostics;
using System.Text.Json;
using Avalonia.Controls;
using Avalonia.VisualTree;
using SHLauncher.Contracts.Engine;
using SHLauncher.UI.ViewModels;
using SHLauncher.UI.Shell;
namespace SHLauncher.UI.Diagnostics;
internal static class ModWorkspaceAudit
{
    public static async Task Run(SHLNWindow window,string output,Action<bool,string> check,Action<string> capture)
    {
        var root=Path.Combine(output,"mod-fixture");var directory=Path.Combine(root,"game","mods");Directory.CreateDirectory(directory);
        var instance=new InstanceInfo("diagnostic-fixture","Mod 界面测试","1.20.1","Fabric",17,4096,"","","",new(root,root,Path.Combine(root,"game")));
        var log=new SHLauncher.Application.Launching.MemoryLauncherLog();
        await using var coordinator=new SHLauncher.Application.Launching.LaunchCoordinator(new FixtureEngine(instance),log);
        await using var launch=new LaunchViewModel(coordinator,log);await launch.InitializeAsync();
        using var gate=new SemaphoreSlim(1,1);
        var service=new SHLauncher.Infrastructure.Content.InstanceModService(new SHLauncher.Platform.RecycleService(),new SHLauncher.Platform.PathOpener(),_=>gate,()=>true,item=>item==instance);
        await using var model=new ModsViewModel(launch,service);var surface=new ModsSurface{DataContext=model};
        window.SetWorkspace(true);window.SetPage(surface,false);
        var timings=new List<object>();var previous=0;
        async Task Ready(int count){for(var i=0;i<200&&(model.Busy||model.Rows.Count!=count);i++)await Task.Delay(25);check(!model.Busy&&model.Rows.Count==count,"Mod UI populated "+count);}
        foreach(var count in new[]{100,300,600})
        {
            for(var i=previous;i<count;i++){using var zip=ZipFile.Open(Path.Combine(directory,$"fixture-{i}.jar"),ZipArchiveMode.Create);using var writer=new StreamWriter(zip.CreateEntry("fabric.mod.json").Open());writer.Write(JsonSerializer.Serialize(new{id="fixture_"+i,name="测试 Mod "+i,version="1.0",authors=new[]{"SHLN test fixture"},depends=new{minecraft="1.20.1"}}));}
            var timer=Stopwatch.StartNew();model.ForceRefreshCommand.Execute(null);await Ready(count);window.UpdateLayout();timings.Add(new{Count=count,ElapsedMilliseconds=timer.Elapsed.TotalMilliseconds,RealizedRows=surface.GetVisualDescendants().OfType<ListBoxItem>().Count()});capture("mods-"+count);previous=count;
        }
        check(surface.GetVisualDescendants().OfType<ListBoxItem>().Count()<100,"600 Mod rows remain virtualized");
        model.Search="fixture_599";check(model.Rows.Count==1,"Mod search matches metadata identity");model.Search="";
        foreach(var row in model.Rows.Take(3))row.Selected=true;
        model.DisableCommand.Execute(null);await model.DisableCommand.DrainAsync();await Ready(600);
        check(Directory.GetFiles(directory,"*.disabled").Length==3,"UI batch disable writes three real file states");
        model.Filter=2;check(model.Rows.Count==3,"disabled filter reflects disk state");model.SelectAllCommand.Execute(null);model.EnableCommand.Execute(null);await model.EnableCommand.DrainAsync();model.Filter=0;await Ready(600);
        check(Directory.GetFiles(directory,"*.disabled").Length==0,"UI batch enable restores files");
        for(var i=0;i<20;i++){model.Deactivate();model.Activate();await Ready(600);}
        var source=Path.Combine(root,"watcher.jar");using(var zip=ZipFile.Open(source,ZipArchiveMode.Create)){using var writer=new StreamWriter(zip.CreateEntry("fabric.mod.json").Open());writer.Write("{\"id\":\"watcher\",\"version\":\"1\"}");}
        await model.InstallAsync([source]);await Ready(601);check(File.Exists(source),"UI local install preserves original");
        model.Search="watcher";model.Rows.Single().Selected=true;var bytes=new FileInfo(Path.Combine(directory,"watcher.jar")).Length;
        await model.RecycleSelectedAsync();model.Search="";await Ready(600);check(!File.Exists(Path.Combine(directory,"watcher.jar")),"Windows recycle removes only selected fixture from source folder");
        await File.WriteAllTextAsync(Path.Combine(output,"mod-ui-metrics.json"),JsonSerializer.Serialize(new{Fixture=true,Timings=timings,Recycled=new{File="watcher.jar",Bytes=bytes},PhysicalExplorerDrop="Not verified"},new JsonSerializerOptions{WriteIndented=true}));
        window.SetPage(new TextBlock{Text="Mod 检查完成"},false);
    }
    private sealed class FixtureEngine(InstanceInfo instance):ILauncherEngine
    {
        public Task<IReadOnlyList<InstanceInfo>> GetInstancesAsync(CancellationToken token)=>Task.FromResult<IReadOnlyList<InstanceInfo>>([instance]);
        public IAsyncEnumerable<EngineUpdate> LaunchAsync(LaunchRequest request,CancellationToken token)=>throw new NotSupportedException("UI fixture never launches Minecraft");
    }
}
