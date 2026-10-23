using SHLauncher.Contracts.Engine;
namespace SHLauncher.Contracts.Runtime;
public sealed record SystemMemory(long TotalMb,long AvailableMb);
public interface IInstanceWorkloadService{Task<int> CountModsAsync(string gameDirectory,CancellationToken token);}
public interface ISystemMemoryService{SystemMemory Read();}
public sealed record MemoryPreference(bool Automatic=true,int CustomMb=4096);
public sealed record MemoryConfiguration(MemoryPreference Global,Dictionary<string,MemoryPreference> Instances,int SchemaVersion=1);
public interface IMemoryPreferenceStore{MemoryConfiguration Read();void Save(MemoryConfiguration configuration);}
public sealed record MemoryResolution(int RequestedMb,int LimitMb,int ResolvedMb,int ModCount,SystemMemory System,string? Problem,string Explanation);
public interface IGameMemoryService
{
    MemoryConfiguration Configuration{get;}
    void Save(MemoryConfiguration configuration);
    Task<MemoryResolution> InspectAsync(InstanceInfo instance,JavaRuntime? java,string gameDirectory,CancellationToken token);
}
