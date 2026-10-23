using System.IO.Compression;
using System.Text;
using System.Text.Json;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Install;
using SHLauncher.Infrastructure.Install;
internal static class PackLayoutScenarios
{
    internal static async Task Run()
    {
        var root=new DirectoryInfo(AppContext.BaseDirectory);while(root is not null&&!File.Exists(Path.Combine(root.FullName,"SHLauncherNext.sln")))root=root.Parent;
        var data=Path.Combine(root!.FullName,".devdata","pack-layout-tests",Guid.NewGuid().ToString("N"));Directory.CreateDirectory(data);
        var archive=Path.Combine(data,"synthetic.zip");
        var local=new[]{"mods/local.jar","resourcepacks/theme.zip","shaderpacks/shader.zip","config/settings.toml","saves/fixture/level.dat","screenshots/fixture.png","tacz/pack.json","options.txt"};
        using(var zip=ZipFile.Open(archive,ZipArchiveMode.Create))
        {
            using(var writer=new StreamWriter(zip.CreateEntry("manifest.json").Open()))writer.Write(JsonSerializer.Serialize(new{manifestType="minecraftModpack",manifestVersion=1,name="fixture",minecraft=new{version="1.20.1",modLoaders=new[]{new{id="forge-47.3.22",primary=true}}},overrides="overrides",files=Array.Empty<object>()}));
            foreach(var path in local){using var writer=new StreamWriter(zip.CreateEntry("overrides/"+path).Open());writer.Write("fixture:"+path);}
        }
        var inspector=new CurseForgePackInspector();var preview=await inspector.InspectAsync(archive,default);
        var downloads=new EmptyDownloads();var recipe=new InstallationRecipe(Guid.NewGuid(),"1.20.1",LoaderType.Forge,"47.3.22","fixture",Guid.NewGuid(),archive,preview.ArchiveSha256);
        var installer=new ManagedInstanceInstallService(data,stage=>new Backend(stage),downloads,new Categories(),inspector,()=>throw new Exception("Local-only pack must not require provider access."));
        var result=await installer.InstallAsync(recipe,null,default);var final=Path.Combine(data,"instances",recipe.InstanceKey.ToString("N"));var game=Path.Combine(final,"game");
        foreach(var path in local)
            if(await File.ReadAllTextAsync(Path.Combine(game,path))!="fixture:"+path)throw new Exception("Wrong override placement: "+path);
        if(Directory.GetFiles(game,"*",SearchOption.AllDirectories).Length!=local.Length||Directory.Exists(Path.Combine(final,"mods"))||Directory.Exists(Path.Combine(game,"overrides"))||!File.Exists(Path.Combine(final,"instance.json")))
            throw new Exception("Unexpected published instance layout.");
        using var cancelled=new CancellationTokenSource();
        var retry=recipe with{Id=Guid.NewGuid(),InstanceKey=Guid.NewGuid()};
        var cancelling=new ManagedInstanceInstallService(data,stage=>new Backend(stage,()=>cancelled.Cancel()),downloads,new Categories(),inspector,()=>throw new Exception("Local-only pack must not require provider access."));
        var stopped=false;try{await cancelling.InstallAsync(retry,null,cancelled.Token);}catch(OperationCanceledException){stopped=true;}
        if(!stopped||Directory.Exists(Path.Combine(data,"instances",retry.InstanceKey.ToString("N"))))throw new Exception("Cancelled staged instance was published.");
        Console.WriteLine("PASS: synthetic local-only pack needs no CF access/metadata; 8-category overrides map exactly to game paths; cancel after staged core never publishes. Not real modpack/launch proof.");
    }
    private sealed class Categories:IJsonMetadataSource
    {public Task<Stream> OpenReadAsync(Uri uri,Guid id,CancellationToken token)=>throw new Exception("Local-only pack must not request provider metadata.");}
    private sealed class EmptyDownloads:IDownloadManager
    {
        public Task<DownloadResult> DownloadAsync(DownloadRequest request,IProgress<DownloadTaskSnapshot>? progress,CancellationToken token)
        {if(request.Files.Count!=0)throw new Exception("Unexpected remote files.");return Task.FromResult(new DownloadResult(request.Id,true,false,[],[]));}
        public IReadOnlyList<DownloadEvent> Events(Guid? id=null)=>[];
    }
    private sealed class Backend(string staging,Action? afterStage=null):IInstallationBackend,IMinecraftVersionService,ILoaderVersionService
    {
        public async Task<InstallResult> InstallAsync(InstallationRecipe recipe,IProgress<InstallProgress>? progress,CancellationToken token)
        {
            var path=Path.Combine(staging,recipe.InstanceKey.ToString("N"),"runtime");Directory.CreateDirectory(path);
            await File.WriteAllTextAsync(Path.Combine(path,"fixture-core"),"fixture",token);afterStage?.Invoke();
            return new(recipe.Id,"fixture-profile",path,1,0,0);
        }
        public Task<IReadOnlyList<MinecraftVersionInfo>> GetVersionsAsync(CancellationToken token)=>Task.FromResult<IReadOnlyList<MinecraftVersionInfo>>([new("1.20.1",MinecraftVersionKind.Release,DateTimeOffset.UtcNow,true)]);
        public Task<IReadOnlyList<LoaderVersionInfo>> GetVersionsAsync(LoaderType loader,string minecraft,CancellationToken token)=>Task.FromResult<IReadOnlyList<LoaderVersionInfo>>([new(loader,minecraft,"47.3.22",true,false,"synthetic catalog")]);
    }
}
