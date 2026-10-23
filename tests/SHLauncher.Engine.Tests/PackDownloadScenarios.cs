using System.IO.Compression;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Install;
using SHLauncher.Downloads;
using SHLauncher.Infrastructure.Install;
internal static class PackDownloadScenarios
{
    internal static async Task Run()
    {
        var root=new DirectoryInfo(AppContext.BaseDirectory);
        while(root is not null&&!File.Exists(Path.Combine(root.FullName,"SHLauncherNext.sln")))root=root.Parent;
        var data=Path.Combine(root!.FullName,".devdata","pack-download-tests",Guid.NewGuid().ToString("N"));
        var cache=Path.Combine(data,"shared","cache");Directory.CreateDirectory(cache);
        var checks=0;void Check(bool value){if(!value)throw new Exception("Pack download/publish regression.");checks++;}
        var content=new[]{("mc-mods","mods","mod.jar"),("texture-packs","resourcepacks","resource.zip"),("shaders","shaderpacks","shader.zip")};
        byte[] Bytes(int index)=>Encoding.UTF8.GetBytes("synthetic-exact-artifact-"+index);
        string Hash(int index)=>Convert.ToHexString(SHA1.HashData(Bytes(index)));
        for(var i=0;i<content.Length;i++)await File.WriteAllBytesAsync(Path.Combine(cache,"sha1-"+Hash(i).ToLowerInvariant()),Bytes(i));
        var archive=Path.Combine(data,"synthetic.zip");
        using(var zip=ZipFile.Open(archive,ZipArchiveMode.Create))
        {
            using(var writer=new StreamWriter(zip.CreateEntry("manifest.json").Open()))
                writer.Write(JsonSerializer.Serialize(new{manifestType="minecraftModpack",manifestVersion=1,name="synthetic",minecraft=new{version="1.20.1",modLoaders=new[]{new{id="forge-47.3.22",primary=true}}},overrides="overrides",files=Enumerable.Range(0,3).Select(i=>new{projectID=100+i,fileID=200+i,required=true})}));
            foreach(var path in new[]{"config/test.toml","scripts/test.zs","saves/test/level.dat","options.txt"})
            {using var writer=new StreamWriter(zip.CreateEntry("overrides/"+path).Open());writer.Write("fixture:"+path);}
        }
        var inspector=new CurseForgePackInspector();var preview=await inspector.InspectAsync(archive,default);
        using var scheduler=new DownloadScheduler();var attempts=0;
        using var manager=new DownloadManager(scheduler,cache,authorize:_=>{Interlocked.Increment(ref attempts);throw new IOException("Synthetic fixture forbids external network.");});
        var source=new Metadata(content,Bytes,Hash);
        var installer=new ManagedInstanceInstallService(data,stage=>new Backend(stage),manager,source,inspector,()=>true);
        var recipe=new InstallationRecipe(Guid.NewGuid(),"1.20.1",LoaderType.Forge,"47.3.22","same name",Guid.NewGuid(),archive,preview.ArchiveSha256);
        string Final(InstallationRecipe value)=>Path.Combine(data,"instances",value.InstanceKey.ToString("N"));
        async Task Validate(InstallationRecipe value)
        {
            var final=Final(value);var game=Path.Combine(final,"game");
            Check(File.Exists(Path.Combine(final,"instance.json"))&&File.Exists(Path.Combine(final,"runtime","fixture-core")));
            for(var i=0;i<content.Length;i++)Check((await File.ReadAllBytesAsync(Path.Combine(game,content[i].Item2,content[i].Item3))).SequenceEqual(Bytes(i)));
            foreach(var path in new[]{"config/test.toml","scripts/test.zs","saves/test/level.dat","options.txt"})Check(await File.ReadAllTextAsync(Path.Combine(game,path))=="fixture:"+path);
            Check(Directory.GetFiles(game,"*",SearchOption.AllDirectories).Length==7&&!Directory.Exists(Path.Combine(game,"overrides")));
        }
        var installed=await installer.InstallAsync(recipe,null,default);await Validate(recipe);
        Check(installed.NetworkBytes==0&&installed.ReusedFiles==3&&attempts==0);
        await File.WriteAllTextAsync(Path.Combine(Final(recipe),"game","user-data"),"preserve");
        var duplicate=false;
        try{await installer.InstallAsync(recipe with{Id=Guid.NewGuid()},null,default);}catch(InvalidOperationException){duplicate=true;}
        Check(duplicate&&await File.ReadAllTextAsync(Path.Combine(Final(recipe),"game","user-data"))=="preserve");
        var separate=recipe with{Id=Guid.NewGuid(),InstanceKey=Guid.NewGuid()};
        await installer.InstallAsync(separate,null,default);await Validate(separate);
        Check(Final(separate)!=Final(recipe)&&attempts==0);
        var bad=recipe with{Id=Guid.NewGuid(),InstanceKey=Guid.NewGuid()};
        await File.WriteAllTextAsync(Path.Combine(cache,"sha1-"+Hash(0).ToLowerInvariant()),"bad cache");
        var failed=false;try{await installer.InstallAsync(bad,null,default);}catch(IOException){failed=true;}
        Check(failed&&!Directory.Exists(Final(bad))&&attempts>0);
        await File.WriteAllBytesAsync(Path.Combine(cache,"sha1-"+Hash(0).ToLowerInvariant()),Bytes(0));
        var retry=bad with{Id=Guid.NewGuid()};await installer.InstallAsync(retry,null,default);await Validate(retry);
        Check(retry.InstanceKey==bad.InstanceKey);
        using var stop=new CancellationTokenSource();
        var cancel=recipe with{Id=Guid.NewGuid(),InstanceKey=Guid.NewGuid()};
        var cancelling=new ManagedInstanceInstallService(data,stage=>new Backend(stage,()=>stop.Cancel()),manager,source,inspector,()=>true);
        var cancelled=false;try{await cancelling.InstallAsync(cancel,null,stop.Token);}catch(OperationCanceledException){cancelled=true;}
        Check(cancelled&&!Directory.Exists(Final(cancel)));
        foreach(var code in new[]{"HTTP401","HTTP403"})
        {
            var rejected=recipe with{Id=Guid.NewGuid(),InstanceKey=Guid.NewGuid()};
            var denied=new ManagedInstanceInstallService(data,stage=>new Backend(stage),new Denied(code),source,inspector,()=>true);
            var blocked=false;try{await denied.InstallAsync(rejected,null,default);}catch(InstallationPreconditionException error){blocked=error.Code=="ProviderAccessRejected";}
            Check(blocked&&!Directory.Exists(Final(rejected)));
            var api=new JsonMetadataSource(new Denied(code),Path.Combine(data,"metadata"));blocked=false;
            try{using var stream=await api.OpenReadAsync(new Uri("https://api.curseforge.com/v1/inert"),Guid.NewGuid(),default);}
            catch(InstallationPreconditionException error){blocked=error.Code=="ProviderAccessRejected";}
            Check(blocked);
        }
        Console.WriteLine($"PASS: {checks} synthetic pack checks; production DownloadManager verified cache, three remote destinations, overrides, publish, reimport preservation/new GUID, corrupt-cache failure/retry, cancel, API/CDN 401/403. No real CF or Minecraft proof.");
    }
    private sealed class Metadata((string Slug,string Folder,string Name)[] content,Func<int,byte[]> bytes,Func<int,string> hash):IJsonMetadataSource
    {
        public Task<Stream> OpenReadAsync(Uri uri,Guid id,CancellationToken token)
        {
            token.ThrowIfCancellationRequested();object data;
            var path=uri.AbsolutePath.Split('/');
            if(uri.AbsolutePath=="/v1/categories")data=content.Select((c,i)=>new{id=i+1,slug=c.Slug});
            else if(path.Length==4){var index=int.Parse(path[3])-100;data=new{id=index+100,gameId=432,classId=index+1};}
            else{var index=int.Parse(path[3])-100;data=new{id=index+200,modId=index+100,isAvailable=true,fileName=content[index].Name,gameVersions=new[]{"1.20.1","Forge"},hashes=new[]{new{algo=1,value=hash(index)}},downloadUrl="https://edge.forgecdn.net/files/inert/"+content[index].Name,fileLength=bytes(index).Length};}
            return Task.FromResult<Stream>(new MemoryStream(JsonSerializer.SerializeToUtf8Bytes(new{data})));
        }
    }
    private sealed class Backend(string staging,Action? staged=null):IInstallationBackend,IMinecraftVersionService,ILoaderVersionService
    {
        public async Task<InstallResult> InstallAsync(InstallationRecipe recipe,IProgress<InstallProgress>? progress,CancellationToken token)
        {
            var path=Path.Combine(staging,recipe.InstanceKey.ToString("N"),"runtime");Directory.CreateDirectory(path);
            await File.WriteAllTextAsync(Path.Combine(path,"fixture-core"),"synthetic",token);staged?.Invoke();
            return new(recipe.Id,"synthetic-profile",path,1,0,0);
        }
        public Task<IReadOnlyList<MinecraftVersionInfo>> GetVersionsAsync(CancellationToken token)=>Task.FromResult<IReadOnlyList<MinecraftVersionInfo>>([new("1.20.1",MinecraftVersionKind.Release,DateTimeOffset.UtcNow,true)]);
        public Task<IReadOnlyList<LoaderVersionInfo>> GetVersionsAsync(LoaderType loader,string minecraft,CancellationToken token)=>Task.FromResult<IReadOnlyList<LoaderVersionInfo>>([new(loader,minecraft,"47.3.22",true,false,"synthetic")]);
    }
    private sealed class Denied(string code):IDownloadManager
    {
        public Task<DownloadResult> DownloadAsync(DownloadRequest request,IProgress<DownloadTaskSnapshot>? progress,CancellationToken token)=>
            Task.FromResult(new DownloadResult(request.Id,false,false,[],[new(request.Files.FirstOrDefault()?.Destination??"inert","CurseForge CDN",code,"Synthetic provider refusal")]));
        public IReadOnlyList<DownloadEvent> Events(Guid? id=null)=>[];
    }
}
