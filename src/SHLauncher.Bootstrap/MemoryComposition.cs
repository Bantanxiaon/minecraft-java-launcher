using SHLauncher.Contracts.Runtime;
using SHLauncher.Application.Runtime;
using SHLauncher.Infrastructure.Runtime;
namespace SHLauncher.Bootstrap;
public sealed partial class LauncherComposition
{
    public async Task<JavaRuntime?> ProbeMemoryJavaAsync(SHLauncher.Contracts.Engine.InstanceInfo instance,CancellationToken token)
    {
        var runtimes=await new JavaRuntimeService().DetectAsync(string.IsNullOrWhiteSpace(ManualJava)?[]:[ManualJava],token);
        return new JavaSelectionService().Select(instance.JavaMajor,System.Runtime.InteropServices.RuntimeInformation.OSArchitecture.ToString().ToLowerInvariant(),string.IsNullOrWhiteSpace(ManualJava)?null:ManualJava,runtimes).Runtime;
    }
    private IGameMemoryService? _memory;
    public IGameMemoryService Memory=>_memory??=new GameMemoryService(new SHLauncher.Platform.SystemMemoryService(),new MemoryPreferenceStore(_workspace),new InstanceWorkloadService());
}
