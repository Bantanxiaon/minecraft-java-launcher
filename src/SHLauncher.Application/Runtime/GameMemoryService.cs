using SHLauncher.Contracts.Engine;
using SHLauncher.Contracts.Runtime;
namespace SHLauncher.Application.Runtime;
public sealed class GameMemoryService(ISystemMemoryService system,IMemoryPreferenceStore preferences,IInstanceWorkloadService workload):IGameMemoryService
{
    public MemoryConfiguration Configuration=>preferences.Read();
    public void Save(MemoryConfiguration configuration) { var total=system.Read().TotalMb;foreach(var pref in configuration.Instances.Values.Append(configuration.Global))if(!pref.Automatic&&pref.CustomMb>Math.Max(512,total-2048))throw new InvalidDataException("自定义内存必须为系统保留至少 2 GiB。");preferences.Save(configuration); }
    public async Task<MemoryResolution> InspectAsync(InstanceInfo instance,JavaRuntime? java,string gameDirectory,CancellationToken token)
    {
        var config=Configuration;var key=InstanceKey(instance);var pref=config.Instances.GetValueOrDefault(key,config.Global);
        var count=await workload.CountModsAsync(gameDirectory,token);
        return Calculate(system.Read(),pref,instance.Loader!="Vanilla",count,java?.Architecture,instance.JavaMajor);
    }
    public static string InstanceKey(InstanceInfo instance)=>instance.Location is {} location?location.VersionDirectory+"|"+instance.Id:instance.Id;
    public static MemoryResolution Calculate(SystemMemory system,MemoryPreference preference,bool modded,int mods,string? javaArchitecture,int javaMajor)
    {
        if(system.TotalMb<=0||system.AvailableMb<0||system.AvailableMb>system.TotalMb)throw new InvalidDataException("系统内存统计无效。");
        // Workload estimate, not a guarantee: heap excludes native buffers, Java and graphics allocations.
        var requested=preference.Automatic?(modded?3072:2048)+Math.Min(9216,((Math.Max(0,mods)+49)/50)*512):preference.CustomMb;
        var reserve=Math.Max(2048,system.TotalMb/10);var nativeReserve=Math.Max(1024,requested/4);
        var limit=(int)Math.Max(0,Math.Min(system.TotalMb-reserve,system.AvailableMb-nativeReserve)/256*256);
        if(javaArchitecture is "x86" or "i386" or "i686" or "32")limit=Math.Min(limit,1408);
        var minimum=javaMajor>=17?2048:1024;var resolved=preference.Automatic?Math.Min(requested,limit):requested;
        var problem=limit<minimum?"当前可用内存或 Java 架构无法满足此实例的最低预算。":!preference.Automatic&&requested>limit?"自定义内存超过当前可用上限，请降低设置或释放其他程序内存。":null;
        return new(requested,limit,resolved,mods,system,problem,"按实例负载估算，并预留系统及 JVM 非堆内存；启动前会重新计算。");
    }
}
