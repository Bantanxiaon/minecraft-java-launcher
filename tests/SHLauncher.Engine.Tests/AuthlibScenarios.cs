using System.Security.Cryptography;
using System.Text.Json;
using SHLauncher.Contracts.Accounts;
using SHLauncher.Contracts.Install;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Downloads;
using SHLauncher.Infrastructure.Accounts;
using SHLauncher.Engine.CmlLib;
using CmlLib.Core.ProcessBuilder;
internal static class AuthlibScenarios
{
    public static async Task Run(string root)
    {
        Directory.CreateDirectory(root);var checks=new List<string>();
        using var scheduler=new DownloadScheduler();
        using var journal=new JsonDownloadEventSink(Path.Combine(root,"logs"));
        using var downloads=new DownloadManager(scheduler,Path.Combine(root,"cache"),eventSink:journal);
        var metadata=new JsonMetadataSource(downloads,Path.Combine(root,"metadata"));
        var service=new AuthlibInjectorService(downloads,metadata,root);
        var first=await service.PrepareAsync(default);Check(File.Exists(first.Path),"official agent cold download");
        await using(var file=File.OpenRead(first.Path))Check(Convert.ToHexString(await SHA256.HashDataAsync(file)).Equals(first.Sha256,StringComparison.OrdinalIgnoreCase),"official metadata SHA256 matches downloaded binary");
        var count=downloads.Events().Count;var warm=await service.PrepareAsync(default);Check(first==warm&&downloads.Events().Count==count,"warm reuse makes no requests after hash verification");
        var offline=new AuthlibInjectorService(downloads,new OfflineMetadata(),root);
        Check(await offline.PrepareAsync(default)==first,"verified pinned binary works with metadata offline");
        await File.WriteAllTextAsync(first.Path,"corrupt fixture");
        try{await offline.PrepareAsync(default);throw new Exception("Corrupt agent reused");}catch(AccountException e) when(e.Code==AccountError.Unavailable){checks.Add("corrupt binary rejected while offline");}
        var repaired=await service.PrepareAsync(default);Check(repaired==first&&new FileInfo(first.Path).Length>1000,"corrupt target repaired through production DownloadManager verified cache");
        var spaced=Path.Combine(root,"auth agent.jar");File.Copy(first.Path,spaced,true);
        var identity=new LaunchIdentity("AgentFixture",Guid.Parse("11111111-1111-4111-8111-111111111111"),"fixture-token","mojang","{}",new("https://auth.example.org/api/"));
        var option=new MLaunchOption();await AccountLaunchMapping.ApplyAsync(option,identity,new FixedAgent(new(spaced,first.Sha256)),default);
        var args=option.ExtraJvmArguments.SelectMany(a=>a.Values).ToArray();
        Check(args.Count(a=>a.StartsWith("-javaagent:"))==1&&args.Contains("-javaagent:"+spaced+"=https://auth.example.org/api/"),"agent path with spaces remains one argument");
        Check(args.Contains("-Dauthlibinjector.noLogFile")&&option.Session!.AccessToken=="fixture-token","launch session token mapped and agent file logging disabled");
        await File.WriteAllTextAsync(spaced,"tampered fixture");
        try{await AccountLaunchMapping.ApplyAsync(new(),identity,new FixedAgent(new(spaced,first.Sha256)),default);throw new Exception("Tampered agent mapped");}catch(AccountException){checks.Add("launch-time SHA256 rejects tampered agent");}
        using(var cancelled=new CancellationTokenSource())
        {cancelled.Cancel();try{await service.PrepareAsync(cancelled.Token);throw new Exception("Cancellation ignored");}catch(OperationCanceledException){checks.Add("cancellation preserved before agent preparation");}}
        var pinned=JsonDocument.Parse(await File.ReadAllBytesAsync(Path.Combine(root,"shared","authlib","current.json")));
        var descriptor=Path.Combine(root,"shared","authlib","current.json");var originalDescriptor=await File.ReadAllBytesAsync(descriptor);
        await File.WriteAllTextAsync(descriptor,"{\"Url\":\"https://authlib-injector.yushi.moe/artifact/test.jar\",\"Sha256\":null}");
        try{await offline.PrepareAsync(default);throw new Exception("Malformed descriptor accepted");}catch(AccountException e) when(e.Code==AccountError.Unavailable){checks.Add("null checksum in local metadata is safely rejected");}
        await File.WriteAllBytesAsync(descriptor,originalDescriptor);
        using(var cancel=new CancellationTokenSource())
        using(var isolatedScheduler=new DownloadScheduler())
        using(var isolatedDownloads=new DownloadManager(isolatedScheduler,Path.Combine(root,"cancel-cache"),eventSink:new CancelOnResponse(cancel)))
        {
            var cancelRoot=Path.Combine(root,"cancel-in-flight");
            var cancelService=new AuthlibInjectorService(isolatedDownloads,new PinnedMetadata(pinned.RootElement.GetProperty("Url").GetString()!,first.Sha256),cancelRoot);
            try{await cancelService.PrepareAsync(cancel.Token);throw new Exception("Official response cancellation ignored");}catch(OperationCanceledException){checks.Add("production DownloadManager cancellation after official HTTP response propagates");}
            Check(!File.Exists(Path.Combine(cancelRoot,"shared","authlib","current.json"))&&!Directory.EnumerateFiles(cancelRoot,"*.jar",SearchOption.AllDirectories).Any(),"cancelled agent never publishes a descriptor or complete jar");
            long Bytes()=>Directory.EnumerateFiles(cancelRoot,"*",SearchOption.AllDirectories).Sum(f=>new FileInfo(f).Length);
            var bytes=Bytes();await Task.Delay(200);Check(Bytes()==bytes,"no background writes after cancelled agent preparation returns");
        }
        pinned.Dispose();
        await File.WriteAllTextAsync(Path.Combine(root,"result.json"),JsonSerializer.Serialize(new{Passed=true,Checks=checks,AgentSha256=first.Sha256,AgentBytes=new FileInfo(first.Path).Length,Events=downloads.Events()}));
        Console.WriteLine($"PASS: {checks.Count} official authlib download/cache/offline/hash/launch/cancellation checks.");
        void Check(bool condition,string name){if(!condition)throw new Exception(name);checks.Add(name);}
    }
    private sealed class OfflineMetadata:IJsonMetadataSource{public Task<Stream> OpenReadAsync(Uri uri,Guid id,CancellationToken token)=>throw new IOException("Offline test fixture");}
    private sealed class FixedAgent(AuthenticationAgent value):IAuthenticationAgentService{public Task<AuthenticationAgent> PrepareAsync(CancellationToken token)=>Task.FromResult(value);}
    private sealed class CancelOnResponse(CancellationTokenSource cancel):IDownloadEventSink
    {public void Write(DownloadEvent entry){if(entry.Kind=="range"&&entry.Message.StartsWith("Response ",StringComparison.Ordinal))cancel.Cancel();}}
    private sealed class PinnedMetadata(string url,string hash):IJsonMetadataSource
    {public Task<Stream> OpenReadAsync(Uri uri,Guid id,CancellationToken token)=>Task.FromResult<Stream>(new MemoryStream(JsonSerializer.SerializeToUtf8Bytes(new{download_url=url,checksums=new{sha256=hash}})));}
}
