using SHLauncher.Bootstrap;
using SHLauncher.Contracts.Content;
internal static class OnlineCatalogLiveScenarios
{
    internal static async Task Run(string workspace)
    {
        await using var owner=new LauncherComposition(workspace);
        using var stop=new CancellationTokenSource(TimeSpan.FromMinutes(2));
        foreach(var provider in owner.OnlineModProviders)
        {
            var hits=await provider.SearchAsync(new("fabric api",ContentType.Mod,"1.20.1","Fabric"),stop.Token);
            Console.WriteLine(provider.Id+" search: "+string.Join("; ",hits.Take(8).Select(h=>h.Id+" "+h.Name)));
            var project=hits.FirstOrDefault(h=>h.Name.Equals("Fabric API",StringComparison.OrdinalIgnoreCase))??throw new Exception(provider.Id+" did not return Fabric API");
            var files=await provider.VersionsAsync(project.Id,new("1.20.1","Fabric",null),stop.Token);
            if(files.Count==0)throw new Exception(provider.Id+" has no compatible files");
            var file=files[0];var exact=await provider.ExactVersionAsync(project.Id,file.Version.VersionId,stop.Token);
            if(file.Sha1!=exact.Sha1||file.Size!=exact.Size||file.Version.Project!=exact.Version.Project)throw new Exception("Exact file mismatch");
            Console.WriteLine($"PASS live {provider.Id}: project {project.Id}, exact version {exact.Version.VersionId}, {files.Count} compatible files, SHA1 and size match.");
        }
    }
}
