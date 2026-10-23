using System.IO.Compression;
using System.Security.Cryptography;
using System.Text.Json;
using SHLauncher.Contracts.Content;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Engine;
using SHLauncher.Infrastructure.Content;
internal static class OnlineModTransactionScenarios
{
    public static async Task Run()
    {
        var root=new DirectoryInfo(AppContext.BaseDirectory);while(root is not null&&!File.Exists(Path.Combine(root.FullName,"SHLauncherNext.sln")))root=root.Parent;
        var folder=Path.Combine(root!.FullName,".devdata","online-mod-tests",Guid.NewGuid().ToString("N"));var game=Path.Combine(folder,"game");Directory.CreateDirectory(game);
        var instance=new InstanceInfo("fixture","Fixture","1.20.1","Fabric",17,4096,"","","",new(folder,folder,game));using var gate=new SemaphoreSlim(1,1);
        var local=new InstanceModService(new NoRecycle(),new NoOpen(),_=>gate,()=>true,_=>true);var download=new DownloadFixture();
        Task<(ContentInstallPlan,IReadOnlyList<OnlineModFile>)> Resolve(OnlineModFile file,InstanceCompatibilityContext context,IReadOnlyList<ResolvedContentVersion> installed,CancellationToken token)=>Task.FromResult<(ContentInstallPlan,IReadOnlyList<OnlineModFile>)>((new([new(file.Version,false,"fixture")],[]),[file]));
        var service=new OnlineModService(download,local,_=>gate,()=>true,Resolve);
        OnlineModFile FileFor(string version)
        {
            using var memory=new MemoryStream();using(var zip=new ZipArchive(memory,ZipArchiveMode.Create,true)){using var writer=new StreamWriter(zip.CreateEntry("fabric.mod.json").Open());writer.Write(JsonSerializer.Serialize(new{id="transaction_test",version}));}
            download.Bytes=memory.ToArray();return new(new(new("Fixture","project"),version,version,ReleaseChannel.Release,["1.20.1"],["Fabric"],[],DateTimeOffset.UtcNow),"mod-"+version+".jar",download.Bytes.Length,Convert.ToHexString(SHA1.HashData(download.Bytes)),new Uri("https://example.org/mod.jar"));
        }
        void Check(bool value,string message){if(!value)throw new Exception(message);}
        var first=FileFor("1");var preview=await service.PreviewAsync(instance,first,default);Check(preview.CanInstall&&!Directory.Exists(Path.Combine(game,"mods")),"preview writes no Mod directory");
        await service.InstallAsync(instance,preview,null,default);var oldPath=Path.Combine(game,"mods","mod-1.jar");var oldBytes=await File.ReadAllBytesAsync(oldPath);Check(File.Exists(oldPath),"verified staging publishes selected file");
        var second=FileFor("2");preview=await service.PreviewAsync(instance,second,default);download.Fail=true;
        try{await service.InstallAsync(instance,preview,null,default);throw new Exception("failed download accepted");}catch(IOException){}
        Check((await File.ReadAllBytesAsync(oldPath)).SequenceEqual(oldBytes),"failed update preserves old bytes");download.Fail=false;
        preview=await service.PreviewAsync(instance,second,default);await File.WriteAllTextAsync(Path.Combine(game,"mods","external.jar"),"external fixture");
        try{await service.InstallAsync(instance,preview,null,default);throw new Exception("stale preview accepted");}catch(IOException){}
        Check(File.Exists(oldPath),"external changes invalidate preview before replacement");
        preview=await service.PreviewAsync(instance,second,default);await service.InstallAsync(instance,preview,null,default);
        Check(!File.Exists(oldPath)&&File.Exists(Path.Combine(game,"mods","mod-2.jar")),"update publishes new version and retires old active file");
        Check(Directory.GetFiles(Path.Combine(game,".shln-mod-transactions"),"mod-1.jar",SearchOption.AllDirectories).Any(p=>(File.ReadAllBytes(p)).SequenceEqual(oldBytes)),"old bytes remain in transaction backup");
        // Simulate process termination after old-file backup and new-file publication, before index commit.
        var third=FileFor("3");var id=Guid.NewGuid();var tx=Path.Combine(game,".shln-mod-transactions",id.ToString("N"));var stage=Path.Combine(tx,"new");var backup=Path.Combine(tx,"backup");Directory.CreateDirectory(stage);Directory.CreateDirectory(backup);
        File.Move(Path.Combine(game,"mods","mod-2.jar"),Path.Combine(backup,"mod-2.jar"));await File.WriteAllBytesAsync(Path.Combine(game,"mods","mod-3.jar"),download.Bytes);
        await File.WriteAllTextAsync(Path.Combine(game,".shln-mod-transaction.json"),JsonSerializer.Serialize(new{Id=id,Targets=new[]{new{PreviousFile="mod-2.jar",NewFile="mod-3.jar",third.Sha1}},HadIndex=true,IndexSha1=new string('a',40)}));
        await new OnlineModService(download,local,_=>gate,()=>true,(_,_,_,_)=>throw new Exception("Offline recovery must not query a provider")).RecoverAsync(instance,default);
        Check(File.Exists(Path.Combine(game,"mods","mod-2.jar"))&&!File.Exists(Path.Combine(game,"mods","mod-3.jar"))&&!File.Exists(Path.Combine(game,".shln-mod-transaction.json")),"restart recovery restores old files before a new preview");
        Console.WriteLine("PASS: online Mod preview isolation, staged publish, failed update preservation, stale directory detection, retained backup, and interrupted transaction recovery.");
    }
    private sealed class NoRecycle:IRecycleService{public Task RecycleAsync(string path,CancellationToken token)=>throw new NotSupportedException();}
    private sealed class NoOpen:IPathOpener{public void OpenFolder(string path){}}
    private sealed class DownloadFixture:IDownloadManager
    {
        public byte[] Bytes=[];public bool Fail;
        public IReadOnlyList<DownloadEvent> Events(Guid? id=null)=>[];
        public async Task<DownloadResult> DownloadAsync(DownloadRequest request,IProgress<DownloadTaskSnapshot>? progress,CancellationToken token)
        {
            if(Fail)return new(request.Id,false,false,[],[new("","","Fixture","Download failed")]);
            foreach(var file in request.Files){if(file.Sha1!=Convert.ToHexString(SHA1.HashData(Bytes)))throw new Exception("fixture hash mismatch");await File.WriteAllBytesAsync(file.Destination,Bytes,token);}
            return new(request.Id,true,false,request.Files.Select(f=>new DownloadFileResult(f.Destination,"fixture",false,Bytes.Length)).ToArray(),[]);
        }
    }
}
