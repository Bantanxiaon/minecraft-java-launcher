using System.Runtime.CompilerServices;
using System.Text.Json;
using SHLauncher.Contracts.Engine;
using SHLauncher.Contracts.Instances;
namespace SHLauncher.Bootstrap;
public sealed partial class LauncherComposition
{
    private sealed class ManagedLibraryEngine(LauncherComposition owner) : ILauncherEngine
    {
        private readonly Dictionary<string,(ManagedInstance Descriptor,string Folder)> _entries = new();
        public async Task<IReadOnlyList<InstanceInfo>> GetInstancesAsync(CancellationToken token)
        {
            var result = new List<InstanceInfo>(); _entries.Clear();
            var root = Path.Combine(owner._workspace, "instances");
            if (!Directory.Exists(root)) return result;
            foreach (var folder in Directory.EnumerateDirectories(root).Take(1000))
            {
                token.ThrowIfCancellationRequested();
                var descriptor = Path.Combine(folder,"instance.json");
                if (!File.Exists(descriptor)) continue;
                try
                {
                    if (new FileInfo(descriptor).Length > 64*1024) throw new InvalidDataException("Oversized descriptor.");
                    await using var stream = File.OpenRead(descriptor);
                    var record = await JsonSerializer.DeserializeAsync<ManagedInstance>(stream,cancellationToken:token).ConfigureAwait(false);
                    if (record is null || record.SchemaVersion != 1 || record.Id == Guid.Empty || Path.GetFileName(folder) != record.Id.ToString("N") ||
                        string.IsNullOrWhiteSpace(record.DisplayName) || record.DisplayName.Length>128 || !Enum.IsDefined(record.Loader) || string.IsNullOrWhiteSpace(record.ProfileId))
                        throw new InvalidDataException("Invalid instance descriptor.");
                    var engine = owner.CreateManagedEngine(record,folder);
                    var profile = (await engine.GetInstancesAsync(token).ConfigureAwait(false)).SingleOrDefault();
                    if (profile is null) throw new InvalidDataException("Published profile is missing.");
                    var id = record.Id.ToString("N"); _entries.Add(id,(record,folder));
                    result.Add(profile with { Id=id, Name=record.DisplayName, Description="", Location=profile.Location is { } location ? location with {GameDirectory=Path.Combine(folder,"game")} : null });
                }
                catch (Exception error) when (error is IOException or JsonException or InvalidDataException)
                {
                    owner.GameLogs.Write(Guid.Empty,GameLogLevel.Warning,"instance-descriptor",Path.GetFileName(folder)+": "+error.GetType().Name);
                    result.Add(new(Path.GetFileName(folder),"无法读取的实例","","Unknown",0,0,"","实例记录无效，文件已保留。","unknown",
                        Problem:new(LauncherErrorCode.InstanceInvalid,"实例记录无法读取","请检查 instance.json；其他实例仍可使用。",error.GetType().Name,false),IsLaunchable:false));
                }
            }
            return result;
        }
        public async IAsyncEnumerable<EngineUpdate> LaunchAsync(LaunchRequest request,[EnumeratorCancellation] CancellationToken token)
        {
            if (!_entries.TryGetValue(request.InstanceId,out var entry)) throw new LauncherEngineException(new(LauncherErrorCode.InstanceNotFound,"找不到实例","请刷新资料库。",null,false));
            if(File.Exists(Path.Combine(entry.Folder,"game",".shln-mod-transaction.json")))throw new LauncherEngineException(new(LauncherErrorCode.InstanceInvalid,"Mod 更新需要恢复","请打开 Mod 管理，选择在线安装中的恢复中断的更新；此操作无需联网，原文件备份已保留。",null,false));
            var engine = owner._instanceAccess.Guard(owner.CreateManagedEngine(entry.Descriptor,entry.Folder),entry.Folder,()=>
            {
                if(File.Exists(Path.Combine(entry.Folder,"game",".shln-mod-transaction.json")))throw new LauncherEngineException(new(LauncherErrorCode.InstanceInvalid,"Mod 更新需要恢复","请先在 Mod 管理中恢复未完成的更新。",null,false));
            });
            await foreach (var update in engine.LaunchAsync(request with { InstanceId=entry.Descriptor.ProfileId },token).ConfigureAwait(false)) yield return update;
        }
    }
    private ILauncherEngine CreateManagedEngine(ManagedInstance record,string folder) => new SHLauncher.Engine.CmlLib.CmlLibLauncherEngine(
        new InstalledInstanceService(record.ProfileId),new SHLauncher.Infrastructure.Runtime.JavaRuntimeService(),new SHLauncher.Application.Runtime.JavaSelectionService(),
        Accounts,GameLogs,Path.Combine(folder,"runtime"),Path.Combine(_workspace,"temp","launch"),[],
        manualJavaProvider:()=>ManualJava,usePersistentGameData:true,gameDirectoryOverride:SHLauncher.Infrastructure.Install.ArchivePathPolicy.Resolve(folder,"game"),authenticationAgent:_authenticationAgent,memory:Memory,memoryInstanceId:record.Id.ToString("N"));
}
