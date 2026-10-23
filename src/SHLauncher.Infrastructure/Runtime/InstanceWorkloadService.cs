using SHLauncher.Contracts.Runtime;
namespace SHLauncher.Infrastructure.Runtime;
public sealed class InstanceWorkloadService:IInstanceWorkloadService
{
    public Task<int> CountModsAsync(string gameDirectory,CancellationToken token)=>Task.Run(()=>
    {
        token.ThrowIfCancellationRequested();var path=Path.Combine(gameDirectory,"mods");
        if(!Directory.Exists(path))return 0;
        if((File.GetAttributes(path)&FileAttributes.ReparsePoint)!=0)throw new IOException("Mod 目录是链接，无法安全估算负载。");
        var count=0;foreach(var file in Directory.EnumerateFiles(path,"*",new EnumerationOptions{RecurseSubdirectories=false,AttributesToSkip=FileAttributes.ReparsePoint}))
        {token.ThrowIfCancellationRequested();if(file.EndsWith(".jar",StringComparison.OrdinalIgnoreCase))count++;if(count>10000)break;}
        return count;
    },token);
}
