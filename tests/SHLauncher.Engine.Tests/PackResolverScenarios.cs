using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using SHLauncher.Contracts.Install;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Infrastructure.Install;
internal static class PackResolverScenarios
{
    internal static async Task Run()
    {
        var source=new Source();
        var preview=new ModpackPreview("inert.zip","hash",1,ModpackFormat.CurseForge,"test","1.20.1",LoaderType.Forge,"47.3.22","overrides/",
            [new("CurseForge",123,456,true),new("CurseForge",124,457,false)],[]);
        var plan=await new CurseForgePackArtifactResolver(source).ResolveAsync(preview,default);
        if(plan.Count!=1||plan[0].ProviderId!="CurseForge"||plan[0].ArtifactId!="123:456"||plan[0].RelativePath!="mods/exact.jar"||source.Queries.Any(q=>q.Contains("457")))
            throw new Exception("Exact identity/category-derived destination or optional handling failed.");
        source.WrongMc=true;
        var wrong=false;try{await new CurseForgePackArtifactResolver(source).ResolveAsync(preview,default);}catch(InvalidDataException){wrong=true;}
        if(!wrong)throw new Exception("Incompatible exact file must not be replaced with latest.");
        source.WrongMc=false;
        source.Distribution=false;var restricted=false;
        try{await new CurseForgePackArtifactResolver(source).ResolveAsync(preview,default);}
        catch(InstallationPreconditionException error){restricted=error.Code=="ProviderDistributionRestricted";}
        if(!restricted)throw new Exception("Explicit provider distribution refusal must block even if a fixture offers a URL.");
        source.Distribution=null;
        var cases = new (Action<JsonObject> Change, string? Code)[] {
            (d=>d["id"]=999,null), (d=>d["modId"]=999,null),
            (d=>d["isAvailable"]=false,"ProviderFileUnavailable"),
            (d=>d["downloadUrl"]=null,"ProviderFileDownloadUnavailable"),
            (d=>d["downloadUrl"]="http://edge.forgecdn.net/inert",null),
            (d=>d["downloadUrl"]="https://user@edge.forgecdn.net/inert",null),
            (d=>d["fileName"]="../escape.jar",null), (d=>d["fileName"]="CON.jar",null),
            (d=>d["fileName"]="file.jar.",null), (d=>d["fileLength"]=0,null),
            (d=>d["hashes"]=new JsonArray(),null),
            (d=>d["hashes"]=JsonNode.Parse("[{\"algo\":1,\"value\":\"bad\"}]"),null),
            (d=>d["gameVersions"]=new JsonArray("1.20.1"),null)
        };
        foreach(var (change,code) in cases)
        {
            source.Mutate=change; var rejected=false;
            try{await new CurseForgePackArtifactResolver(source).ResolveAsync(preview,default);}
            catch(InvalidDataException) when(code is null){rejected=true;}
            catch(InstallationPreconditionException error) when(error.Code==code){rejected=true;}
            if(!rejected)throw new Exception("Unsafe exact file fixture accepted.");
        }
        source.Mutate=null;
        foreach(var (slug,folder) in new[]{("mc-mods","mods"),("texture-packs","resourcepacks"),("shaders","shaderpacks")})
        {
            source.ClassSlug=slug;
            var resolved=await new CurseForgePackArtifactResolver(source).ResolveAsync(preview,default);
            if(resolved.Single().RelativePath!=folder+"/exact.jar")throw new Exception("Wrong provider category destination.");
        }
        var queries=source.Queries.Count; var invalidIdentity=false;
        try{await new CurseForgePackArtifactResolver(source).ResolveAsync(preview with{RemoteFiles=[new("Other",123,456,true)]},default);}
        catch(InvalidDataException){invalidIdentity=true;}
        if(!invalidIdentity||source.Queries.Count!=queries)throw new Exception("Cross-provider identity was sent to CF.");
        var root=Path.Combine(new DirectoryInfo(AppContext.BaseDirectory).FullName,"unused");
        var installer=new ManagedInstanceInstallService(root,_=>throw new Exception("Base installer must not run without provider access"),
            new NoDownloads(),source,new Inspector(preview),()=>false);
        var blocked=false;try{await installer.InstallAsync(new(Guid.NewGuid(),"1.20.1",LoaderType.Forge,"47.3.22","test",Guid.NewGuid(),"inert.zip","hash"),null,default);}catch(InstallationPreconditionException e){blocked=e.Code=="ProviderAccessMissing";}
        if(!blocked||Directory.Exists(root))throw new Exception("Missing own provider access must stop before instance mutation.");
        Console.WriteLine("PASS: provider identity, metadata class destination, optional skip, wrong-MC refusal, no-key leaves no instance.");
        Console.WriteLine("PASS: 18 additional exact-file negative/category/provider cases including explicit distribution refusal; fixture metadata only, no real CF response.");
    }
    private sealed class Source:IJsonMetadataSource
    {
        internal bool WrongMc;internal List<string> Queries=[];
        internal string ClassSlug="mc-mods";
        internal bool? Distribution;
        internal Action<JsonObject>? Mutate;
        public Task<Stream> OpenReadAsync(Uri uri,Guid sessionId,CancellationToken token)
        {
            Queries.Add(uri.AbsoluteUri);string value;
            if(uri.AbsolutePath.EndsWith("/categories",StringComparison.Ordinal))value=JsonSerializer.Serialize(new{data=new[]{new{id=71,slug=ClassSlug}}});
            else if(uri.AbsolutePath.EndsWith("/mods/123",StringComparison.Ordinal))value=JsonSerializer.Serialize(new{data=new{id=123,gameId=432,classId=71,allowModDistribution=Distribution}});
            else value=JsonSerializer.Serialize(new{data=new{id=456,modId=123,isAvailable=true,fileName="exact.jar",gameVersions=new[]{WrongMc?"1.21":"1.20.1","Forge"},hashes=new[]{new{algo=1,value=new string('a',40)}},downloadUrl="https://edge.forgecdn.net/files/inert/exact.jar",fileLength=1}});
            if(uri.AbsolutePath.Contains("/files/",StringComparison.Ordinal)&&Mutate is not null)
            {var json=JsonNode.Parse(value)!;Mutate(json["data"]!.AsObject());value=json.ToJsonString();}
            return Task.FromResult<Stream>(new MemoryStream(Encoding.UTF8.GetBytes(value)));
        }
    }
    private sealed class Inspector(ModpackPreview value):IModpackInspector{public Task<ModpackPreview> InspectAsync(string path,CancellationToken token)=>Task.FromResult(value);}
    private sealed class NoDownloads:IDownloadManager
    {
        public Task<DownloadResult> DownloadAsync(DownloadRequest r,IProgress<DownloadTaskSnapshot>? p,CancellationToken t)=>throw new Exception("Unexpected content network.");
        public IReadOnlyList<DownloadEvent> Events(Guid? id=null)=>[];
    }
}
