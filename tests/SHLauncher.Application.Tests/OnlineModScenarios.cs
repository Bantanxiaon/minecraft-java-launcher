using System.Text;
using SHLauncher.Contracts.Content;
using SHLauncher.Contracts.Install;
using SHLauncher.Infrastructure.Content;
using SHLauncher.Application.Content;
internal static class OnlineModScenarios
{
    public static async Task Run()
    {
        void Check(bool value,string reason){if(!value)throw new Exception(reason);}
        var context=new InstanceCompatibilityContext("1.20.1","Fabric",null);
        var provider=new FixtureProvider();var planner=new ModDependencyPlanner([provider]);
        OnlineModFile File(string project,string version,IReadOnlyList<DependencyEdge>? deps=null)=>new(new(new("Fixture",project),version,version,ReleaseChannel.Release,["1.20.1"],["fabric"],deps??[],DateTimeOffset.UtcNow),project+".jar",20,new string('a',40),new Uri("https://example.org/file.jar"));
        DependencyEdge Edge(string target,string? exact=null,DependencyKind kind=DependencyKind.Required)=>new(new("Fixture",target),kind,new(exact,null,null));
        provider.Files=[File("dependency","1")];var root=File("root","1",[Edge("dependency")]);
        var success=await planner.PlanAsync(root,context,[],default);Check(success.Plan.CanInstall&&success.Files.Count==2,"required dependency included before writes");
        provider.Files=[];var missing=await planner.PlanAsync(root,context,[],default);Check(!missing.Plan.CanInstall,"missing required dependency blocks");
        provider.Files=[File("dependency","1")];var exact=await planner.PlanAsync(File("root","1",[Edge("dependency","2")]),context,[],default);Check(!exact.Plan.CanInstall,"missing exact version never replaced with latest");
        var incompatible=await planner.PlanAsync(File("root","1",[Edge("dependency",null,DependencyKind.Incompatible)]),context,[provider.Files[0].Version],default);Check(!incompatible.Plan.CanInstall,"installed conflict blocks plan");
        var wrong=await planner.PlanAsync(root,context with{Loader="Forge"},[],default);Check(!wrong.Plan.CanInstall,"wrong loader blocks plan");
        provider.Files=[File("dependency","2")];var breakExisting=await planner.PlanAsync(File("dependency","2"),context,[File("existing","1",[Edge("dependency","1")]).Version],default);Check(!breakExisting.Plan.CanInstall,"update cannot break installed exact dependency");
        provider.Files=[File("a","1",[Edge("b")]),File("b","1",[Edge("a")])];var cycle=await planner.PlanAsync(provider.Files[0],context,[],default);Check(cycle.Plan.CanInstall&&cycle.Files.Count==2,"cycle terminates with unique identities");
        var metadata=new FixtureMetadata();var modrinth=new OnlineModProvider("Modrinth",metadata,()=>true);
        var versions=await modrinth.VersionsAsync("project",context,default);Check(versions.Count==1&&versions[0].Sha1==new string('a',40),"provider maps verified file metadata");
        metadata.Unsafe=true;try{await modrinth.VersionsAsync("project",context,default);throw new Exception("unsafe name accepted");}catch(InvalidDataException){}
        metadata.Unsafe=false;metadata.Restricted=true;var curse=new OnlineModProvider("CurseForge",metadata,()=>true);
        try{await curse.VersionsAsync("123",context,default);throw new Exception("restricted distribution accepted");}catch(IOException){}
        Check(!metadata.FileRequested,"restricted distribution stops before file requests");
        Console.WriteLine("PASS: online provider filename/hash/identity, author distribution restriction, required/exact dependencies, cycles, incompatible installed content, and update dependency preservation.");
    }
    private sealed class FixtureProvider:IOnlineModProvider
    {
        public string Id=>"Fixture";public IReadOnlyList<OnlineModFile> Files=[];
        public Task<IReadOnlyList<ContentSearchResult>> SearchAsync(ContentSearchQuery query,CancellationToken token)=>Task.FromResult<IReadOnlyList<ContentSearchResult>>([]);
        public Task<IReadOnlyList<OnlineModFile>> VersionsAsync(string id,InstanceCompatibilityContext context,CancellationToken token)=>Task.FromResult<IReadOnlyList<OnlineModFile>>(Files.Where(f=>f.Version.Project.ProjectId==id).ToArray());
        public Task<OnlineModFile> ExactVersionAsync(string id,string version,CancellationToken token)=>Task.FromResult(Files.FirstOrDefault(f=>f.Version.Project.ProjectId==id&&f.Version.VersionId==version)??throw new IOException("Exact version unavailable"));
    }
    private sealed class FixtureMetadata:IJsonMetadataSource
    {
        public bool Unsafe,Restricted,FileRequested;
        public Task<Stream> OpenReadAsync(Uri uri,Guid sessionId,CancellationToken token)
        {
            string json;
            if(uri.Host=="api.curseforge.com"){FileRequested|=uri.AbsolutePath.Contains("files");json="{\"data\":{\"id\":123,\"gameId\":432,\"classId\":6,\"allowModDistribution\":"+(Restricted?"false":"true")+"}}";}
            else if(uri.AbsolutePath.EndsWith("/version"))json="[{\"project_id\":\"project\",\"id\":\"v1\",\"version_number\":\"1\",\"game_versions\":[\"1.20.1\"],\"loaders\":[\"fabric\"],\"date_published\":\"2026-01-01T00:00:00Z\",\"version_type\":\"release\",\"dependencies\":[],\"files\":[{\"filename\":\""+(Unsafe?"../escape.jar":"test.jar")+"\",\"primary\":true,\"size\":20,\"hashes\":{\"sha1\":\""+new string('a',40)+"\"},\"url\":\"https://cdn.modrinth.com/file.jar\"}]}]";
            else json="{\"id\":\"project\",\"project_type\":\"mod\"}";
            return Task.FromResult<Stream>(new MemoryStream(Encoding.UTF8.GetBytes(json)));
        }
    }
}
