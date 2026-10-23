using SHLauncher.Bootstrap;
using SHLauncher.Infrastructure.Runtime;
using SHLauncher.Contracts.Instances;
using SHLauncher.Contracts.Install;
using System.Text.Json;
internal static class LibraryRestoreScenarios
{
    internal static async Task Run(string parent)
    {
        var root=Path.Combine(parent,Guid.NewGuid().ToString("N"));Directory.CreateDirectory(root);
        void Check(bool value,string message){if(!value)throw new Exception(message);Console.WriteLine("PASS "+message);}
        var workspace=Path.Combine(root,"managed");var ids=new[]{Guid.NewGuid(),Guid.NewGuid()};
        foreach(var id in ids)
        {
            var folder=Path.Combine(workspace,"instances",id.ToString("N"));Directory.CreateDirectory(Path.Combine(folder,"runtime","versions","fixture"));Directory.CreateDirectory(Path.Combine(folder,"game"));
            await File.WriteAllTextAsync(Path.Combine(folder,"instance.json"),JsonSerializer.Serialize(new ManagedInstance(id,"Fixture "+id,"fixture",LoaderType.Vanilla,"","fixture")));
        }
        // Deliberately incomplete profiles stay visible; selecting them must never launch a game.
        await using(var first=new LauncherComposition(workspace))
        {
            await first.InitializePresentationLibraryAsync(default);
            Check(first.Coordinator.Instances.Count==2,"managed library restored without completed-download replay");
            Check(first.Coordinator.SelectInstance(ids[1].ToString("N")),"select second stable identity");
        }
        await using(var second=new LauncherComposition(workspace))
        {
            await second.InitializePresentationLibraryAsync(default);
            Check(second.Coordinator.Selected?.Id==ids[1].ToString("N"),"selected managed identity survives owner restart");
        }
        var external=Path.Combine(root,"external");Directory.CreateDirectory(Path.Combine(external,"versions","alpha"));Directory.CreateDirectory(Path.Combine(external,"versions","beta"));
        await using(var first=new LauncherComposition(workspace)){await first.InitializePresentationLibraryAsync(default);await first.UseRootAsync(external,default);Check(first.Coordinator.SelectInstance("beta"),"external selection available");}
        await using(var second=new LauncherComposition(workspace)){await second.InitializePresentationLibraryAsync(default);Check(second.Root==external&&second.Coordinator.Selected?.Id=="beta","external root and selection survive restart");}
        var store=new LibraryPreferenceStore(workspace);store.Save(new(Path.Combine(root,"unavailable"),"beta",false));var pref=Path.Combine(workspace,"settings","library.json");var before=await File.ReadAllTextAsync(pref);
        await using(var owner=new LauncherComposition(workspace)){await owner.InitializePresentationLibraryAsync(default);}
        Check(await File.ReadAllTextAsync(pref)==before,"missing external root does not overwrite saved preference");
        await File.WriteAllTextAsync(pref,"{invalid");
        await using(var owner=new LauncherComposition(workspace)){await owner.InitializePresentationLibraryAsync(default);}
        Check(await File.ReadAllTextAsync(pref)=="{invalid","invalid preference remains preserved for diagnosis");
    }
}
