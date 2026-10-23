using System.Collections.Concurrent;
using SHLauncher.Contracts.Content;
using SHLauncher.Contracts.Engine;
namespace SHLauncher.Infrastructure.Content;
public sealed class InstanceModService(IRecycleService recycle,IPathOpener opener,Func<InstanceInfo,SemaphoreSlim> access,Func<bool> mayEdit,Func<InstanceInfo,bool> ownsInstance):IInstanceModService
{
    private readonly ConcurrentDictionary<string,(long Size,DateTime Stamp,LocalMod Mod)> _cache=new(OperatingSystem.IsWindows()?StringComparer.OrdinalIgnoreCase:StringComparer.Ordinal);
    public int ClearMetadataCache(){var count=_cache.Count;_cache.Clear();return count;}
    public bool CanManage(InstanceInfo instance)=>ownsInstance(instance);
    private static string Folder(InstanceInfo instance)
    {
        var root=instance.Location?.GameDirectory??throw new IOException("实例没有可用的游戏目录。");
        if(!Path.IsPathFullyQualified(root))throw new IOException("实例目录必须为绝对路径。");
        var path=Path.GetFullPath(Path.Combine(root,"mods"));CheckLinks(path);return path;
    }
    private static void CheckLinks(string path)
    {
        for(var item=path;!string.IsNullOrEmpty(item);item=Path.GetDirectoryName(item))
            if((File.Exists(item)||Directory.Exists(item))&&(File.GetAttributes(item)&FileAttributes.ReparsePoint)!=0)throw new IOException("链接目录或文件不能用于 Mod 文件操作。");
    }
    private static string FilePath(string folder,string name)
    {
        if(string.IsNullOrWhiteSpace(name)||name!=Path.GetFileName(name)||name.IndexOfAny(Path.GetInvalidFileNameChars())>=0||name.Contains(':')||(!name.EndsWith(".jar",StringComparison.OrdinalIgnoreCase)&&!name.EndsWith(".jar.disabled",StringComparison.OrdinalIgnoreCase)))throw new IOException("Mod 文件名无效。");
        var path=Path.Combine(folder,name);CheckLinks(path);return path;
    }
    public async Task<IReadOnlyList<LocalMod>> ScanAsync(InstanceInfo instance,bool force,CancellationToken token)
    {
        var folder=Folder(instance);if(!Directory.Exists(folder))return [];
        return await Task.Run(()=>
        {
            var result=new List<LocalMod>();var seen=new HashSet<string>(StringComparer.OrdinalIgnoreCase);
            foreach(var path in Directory.EnumerateFiles(folder).Take(10000))
            {
                token.ThrowIfCancellationRequested();var name=Path.GetFileName(path);
                if(!name.EndsWith(".jar",StringComparison.OrdinalIgnoreCase)&&!name.EndsWith(".jar.disabled",StringComparison.OrdinalIgnoreCase))continue;
                try
                {
                    FilePath(folder,name);var info=new FileInfo(path);seen.Add(path);
                    if(!force&&_cache.TryGetValue(path,out var cached)&&cached.Size==info.Length&&cached.Stamp==info.LastWriteTimeUtc){result.Add(cached.Mod);continue;}
                    var mod=ModMetadataReader.Read(path);_cache[path]=(info.Length,info.LastWriteTimeUtc,mod);result.Add(mod);
                }
                catch(IOException){result.Add(new(name,path,name.EndsWith(".jar",StringComparison.OrdinalIgnoreCase),"",name,"未知","未知",[],"",[],null,"文件暂不可读取，已保留。",0,default));}
            }
            foreach(var key in _cache.Keys.Where(k=>Path.GetDirectoryName(k)==folder&&!seen.Contains(k)))_cache.TryRemove(key,out _);
            return (IReadOnlyList<LocalMod>)result.OrderBy(m=>m.Name,StringComparer.OrdinalIgnoreCase).ToArray();
        },token);
    }
    private async Task<IReadOnlyList<ModOperationResult>> Mutate(InstanceInfo instance,IReadOnlyList<string> files,Func<string,string,Task<string>> operation,CancellationToken token)
    {
        if(!CanManage(instance))throw new IOException("此实例为只读连接，不能修改原目录。");
        if(!mayEdit())throw new IOException("游戏任务进行中，请结束游戏后再修改 Mod。");
        var gate=access(instance);if(!await gate.WaitAsync(0,token))throw new IOException("实例正在使用，暂不能修改 Mod。");
        try
        {
            if(!CanManage(instance))throw new IOException("此实例为只读连接，不能修改原目录。");
        if(!CanManage(instance)||!mayEdit())throw new IOException("实例正在使用或已失去管理权限。");var folder=Folder(instance);if(File.Exists(Path.Combine(Path.GetDirectoryName(folder)!,".shln-mod-transaction.json")))throw new IOException("存在未完成的在线 Mod 更新，请先恢复后再修改。");Directory.CreateDirectory(folder);var result=new List<ModOperationResult>();
            foreach(var file in files.Distinct(StringComparer.OrdinalIgnoreCase))
            {
                token.ThrowIfCancellationRequested();try{var message=await operation(folder,file);result.Add(new(Path.GetFileName(file),true,message));}
                catch(Exception e) when(e is IOException or UnauthorizedAccessException or InvalidDataException or OperationCanceledException or PlatformNotSupportedException){result.Add(new(Path.GetFileName(file),false,e.Message));}
            }
            return result;
        }
        finally{gate.Release();}
    }
    public Task<IReadOnlyList<ModOperationResult>> SetEnabledAsync(InstanceInfo instance,IReadOnlyList<string> files,bool enabled,CancellationToken token)=>Mutate(instance,files,(folder,name)=>
    {
        var source=FilePath(folder,name);if(!File.Exists(source))throw new FileNotFoundException("文件已变化，请刷新。",name);
        var disabled=name.EndsWith(".disabled",StringComparison.OrdinalIgnoreCase);if(enabled==!disabled)return Task.FromResult("状态无需更改");
        var destination=FilePath(folder,enabled?name[..^9]:name+".disabled");if(File.Exists(destination))throw new IOException("目标文件已存在，未覆盖。");
        File.Move(source,destination);_cache.TryRemove(source,out _);return Task.FromResult(enabled?"已启用":"已禁用");
    },token);
    public Task<IReadOnlyList<ModOperationResult>> RecycleAsync(InstanceInfo instance,IReadOnlyList<string> files,CancellationToken token)=>Mutate(instance,files,async(folder,name)=>
    {var path=FilePath(folder,name);await recycle.RecycleAsync(path,token);_cache.TryRemove(path,out _);return "已移入回收站";},token);
    public Task<IReadOnlyList<ModOperationResult>> InstallLocalAsync(InstanceInfo instance,IReadOnlyList<string> paths,CancellationToken token)=>Mutate(instance,paths,async(folder,source)=>
    {
        if(!Path.IsPathFullyQualified(source)||!source.EndsWith(".jar",StringComparison.OrdinalIgnoreCase))throw new IOException("请选择本地 JAR 文件。");CheckLinks(source);
        var parsed=await Task.Run(()=>ModMetadataReader.Read(source),token);if(parsed.Problem is not null)throw new InvalidDataException(parsed.Problem);
        var destination=FilePath(folder,Path.GetFileName(source));if(File.Exists(destination)||File.Exists(destination+".disabled"))throw new IOException("已存在同名 Mod，未覆盖。请先处理冲突。");
        var staged=destination+"."+Guid.NewGuid().ToString("N")+".pending";CheckLinks(staged);
        await using(var input=new FileStream(source,FileMode.Open,FileAccess.Read,FileShare.Read))await using(var output=new FileStream(staged,FileMode.CreateNew,FileAccess.Write,FileShare.None)){await input.CopyToAsync(output,token);await output.FlushAsync(token);}
        token.ThrowIfCancellationRequested();var verified=ModMetadataReader.Read(staged);if(verified.Problem is not null)throw new InvalidDataException(verified.Problem);File.Move(staged,destination);return "已安装，源文件保留";
    },token);
    public void OpenFolder(InstanceInfo instance){var folder=Folder(instance);if(!Directory.Exists(folder)){if(!CanManage(instance))throw new IOException("只读实例没有 Mod 目录。");Directory.CreateDirectory(folder);}opener.OpenFolder(folder);}
    public IDisposable Watch(InstanceInfo instance,Action changed)
    {
        var folder=Folder(instance);if(!Directory.Exists(folder)){if(!CanManage(instance))return new EmptyWatch();Directory.CreateDirectory(folder);}return new Watcher(folder,changed);
    }
    private sealed class EmptyWatch:IDisposable{public void Dispose(){}}
    private sealed class Watcher:IDisposable
    {
        private readonly FileSystemWatcher _watcher;private readonly Timer _timer;private bool _disposed;
        public Watcher(string folder,Action changed)
        {
            _timer=new(_=>{if(!_disposed)changed();},null,Timeout.Infinite,Timeout.Infinite);
            _watcher=new(folder){NotifyFilter=NotifyFilters.FileName|NotifyFilters.LastWrite|NotifyFilters.Size,IncludeSubdirectories=false};
            _watcher.Changed+=Event;_watcher.Created+=Event;_watcher.Deleted+=Event;_watcher.Renamed+=Event;_watcher.Error+=(_,_)=>Signal();_watcher.EnableRaisingEvents=true;
        }
        private void Event(object sender,FileSystemEventArgs e)=>Signal();
        private void Signal(){try{if(!_disposed)_timer.Change(250,Timeout.Infinite);}catch(ObjectDisposedException){}}
        public void Dispose(){_disposed=true;_watcher.Dispose();_timer.Dispose();}
    }
}
