using SHLauncher.Application.Runtime;
using SHLauncher.Contracts.Runtime;
using SHLauncher.Infrastructure.Runtime;
internal static class MemoryScenarios
{
    public static void Run()
    {
        void Check(bool value,string name){if(!value)throw new Exception(name);}
        var system=new SystemMemory(32768,20000);
        var vanilla=GameMemoryService.Calculate(system,new(),false,0,"x64",21);
        var mods=GameMemoryService.Calculate(system,new(),true,300,"x64",21);
        Check(mods.ResolvedMb>vanilla.ResolvedMb,"workload affects automatic allocation");
        Check(GameMemoryService.Calculate(new(8192,1500),new(),true,300,"x64",21).Problem is not null,"low available RAM blocks unsafe launch");
        var custom=GameMemoryService.Calculate(new(8192,4096),new(false,8192),false,0,"x64",21);
        Check(custom.RequestedMb==8192&&custom.ResolvedMb==8192&&custom.LimitMb<8192&&custom.Problem is not null,"oversized custom value preserved and blocked, never silently clamped");
        Check(GameMemoryService.Calculate(system,new(false,4096),true,0,"x86",17).Problem is not null,"32-bit Java cannot satisfy modern instance budget");
        var root=new DirectoryInfo(AppContext.BaseDirectory);while(root is not null&&!File.Exists(Path.Combine(root.FullName,"SHLauncherNext.sln")))root=root.Parent;
        var folder=Path.Combine(root!.FullName,".devdata","memory-tests",Guid.NewGuid().ToString("N"));var store=new MemoryPreferenceStore(folder);
        var config=new MemoryConfiguration(new(false,3072),new(){["instance-one"]=new(false,6144)});store.Save(config);
        var restored=new MemoryPreferenceStore(folder).Read();Check(restored.Global.CustomMb==3072&&restored.Instances["instance-one"].CustomMb==6144,"single global and override store persists after reopen");
        Console.WriteLine("PASS: memory workload, OS budget, custom overflow, x86 guard, restart global and instance override persistence.");
    }
}
