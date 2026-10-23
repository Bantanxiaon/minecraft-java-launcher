using Avalonia.VisualTree;
using SHLauncher.Contracts.Content;
using SHLauncher.Contracts.Engine;
using SHLauncher.UI.Shell;
using SHLauncher.UI.ViewModels;
namespace SHLauncher.UI.Diagnostics;
internal static class OnlineModUiAudit
{
    internal static async Task Run(SHLNWindow window,string output,IReadOnlyList<IOnlineModProvider> providers,Action<bool,string> check,Action<string> capture)
    {
        var root=Path.Combine(output,"online-game-fixture");var game=Path.Combine(root,"game");Directory.CreateDirectory(game);
        var instance=new InstanceInfo("online-fixture","在线 Mod 实测","1.20.1","Fabric",17,4096,"","","",new(root,root,game));
        using var gate=new SemaphoreSlim(1,1);using var scheduler=new SHLauncher.Downloads.DownloadScheduler();using var downloads=new SHLauncher.Downloads.DownloadManager(scheduler,Path.Combine(root,"cache"));
        var local=new SHLauncher.Infrastructure.Content.InstanceModService(new SHLauncher.Platform.RecycleService(),new SHLauncher.Platform.PathOpener(),_=>gate,()=>true,i=>i==instance);
        var service=new SHLauncher.Infrastructure.Content.OnlineModService(downloads,local,_=>gate,()=>true,new SHLauncher.Application.Content.ModDependencyPlanner(providers).PlanAsync);
        await using var model=new OnlineModsViewModel(providers,service);model.Activate(instance);model.Query="fabric api";
        var sheet=window.ShowSheetAsync("在线 Mod 实测",new OnlineModsSurface{DataContext=model},true);await Task.Delay(200);
        model.SearchCommand.Execute(null);await model.SearchCommand.DrainAsync();check(model.Projects.Count>0,"online UI search returns real projects");
        model.Project=model.Projects.First(p=>p.Name.Equals("Fabric API",StringComparison.OrdinalIgnoreCase));model.VersionsCommand.Execute(null);await model.VersionsCommand.DrainAsync();check(model.Versions.Count>1,"online UI lists real compatible versions");
        var latest=model.Versions[0];model.Version=model.Versions[1];model.PreviewCommand.Execute(null);await model.PreviewCommand.DrainAsync();check(model.CanInstall,"real Mod dependency preview is installable");capture("online-mod-plan");
        model.InstallCommand.Execute(null);await model.InstallCommand.DrainAsync();check(File.Exists(Path.Combine(game,"mods",model.Version.FileName)),"online UI installs real verified Mod JAR");
        var previous=model.Version.FileName;model.Version=latest;model.PreviewCommand.Execute(null);await model.PreviewCommand.DrainAsync();check(model.CanInstall,"online UI plans tracked project update");
        model.InstallCommand.Execute(null);await model.InstallCommand.DrainAsync();check(File.Exists(Path.Combine(game,"mods",latest.FileName))&&!File.Exists(Path.Combine(game,"mods",previous)),"online UI replaces tracked version after verification");
        check(Directory.GetFiles(Path.Combine(game,".shln-mod-transactions"),previous,SearchOption.AllDirectories).Length==1,"real update retains previous JAR backup");capture("online-mod-installed");
        window.GetVisualDescendants().OfType<Ursa.Controls.OverlayShared.OverlayFeedbackElement>().Last().Close();await sheet;
    }
}
