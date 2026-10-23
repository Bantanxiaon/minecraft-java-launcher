using System.Net;
using System.Text;
using System.Text.Json;
using SHLauncher.Application.Accounts;
using SHLauncher.Contracts.Accounts;
using SHLauncher.Downloads;
using SHLauncher.Infrastructure.Accounts;
using SHLauncher.Platform;
using SHLauncher.Engine.CmlLib;
using CmlLib.Core.ProcessBuilder;
internal static class AccountScenarios
{
    private static int _checks;
    public static async Task Run()
    {
        var root=Path.Combine(Path.GetTempPath(),"shln-accounts-"+Guid.NewGuid().ToString("N"));Directory.CreateDirectory(root);
        Check(OfflineIdentity.Uuid("Steve")==Guid.Parse("5627dd98-e6be-3c21-b8a8-e92344183641"),"standard offline UUID vector");
        Check(OfflineIdentity.Uuid("Steve")!=OfflineIdentity.Uuid("steve"),"offline case identity");
        var fake=new Protocol();var ygg=new YggdrasilAccountProvider(fake);
        var credentials=new ProtectedCredentialStore(root,new WindowsCredentialProtector());
        var store=new JsonAccountCatalogStore(root);var canChange=true;
        using var accounts=new AccountCoordinator(store,credentials,[new OfflineMinecraftAccountProvider(),ygg,new MicrosoftAccountProvider()],ygg,()=>canChange);
        Check(!await accounts.InitializeAsync(default),"first run requires account");
        foreach(var name in new[]{"a","bad name","玩家","abcdefghijklmnopq","a\nb"})await Error(AccountError.InvalidInput,()=>accounts.LoginAsync(MinecraftAccountType.Offline,null,name,"",default));
        Check(await accounts.LoginAsync(MinecraftAccountType.Offline,null,"Steve","",default),"offline login");
        var first=accounts.Active!;var identity=await accounts.GetAsync(default);
        Check(identity.ProfileId==first.ProfileId&&identity.AccessToken=="0"&&identity.Authority is null,"normalized offline launch identity");
        Check(!Directory.Exists(Path.Combine(root,"accounts","private")),"offline needs no credential store");
        await accounts.LoginAsync(MinecraftAccountType.Offline,null,"Steve","",default);
        Check(accounts.Accounts.Count==1&&accounts.Active!.AccountId==first.AccountId,"offline re-add deduplicates stable identity");
        using(var restart=new AccountCoordinator(store,credentials,[new OfflineMinecraftAccountProvider(),ygg,new MicrosoftAccountProvider()],ygg,()=>true))
        {Check(await restart.InitializeAsync(default),"restore active offline");Check((await restart.GetAsync(default)).ProfileId==first.ProfileId,"restart preserves UUID");}
        canChange=false;await Error(AccountError.Busy,()=>accounts.LogoutAsync(first.AccountId,true,default));canChange=true;
        await Error(AccountError.Unavailable,()=>accounts.LoginAsync(MinecraftAccountType.Microsoft,null,"","",default));
        var authority=await accounts.AddAuthorityAsync("https://auth.example.org/api/",default);
        Check(authority.DisplayName=="Local protocol fixture"&&authority.ApiRoot.AbsolutePath=="/api/","metadata discovery");
        var same=await accounts.AddAuthorityAsync("https://auth.example.org/api/",default);Check(same.ProviderId==authority.ProviderId&&accounts.Authorities.Count==1,"authority dedup");authority=same;
        fake.ManyProfiles=true;Check(!await accounts.LoginAsync(MinecraftAccountType.Yggdrasil,authority,"fixture-user","fixture-password",default),"multi-profile waits for selection");
        Check(accounts.PendingProfiles.Count==2&&accounts.Active!.AccountId==first.AccountId,"pending profiles do not replace active identity");
        var profile=accounts.PendingProfiles[1];await accounts.SelectProfileAsync(profile,default);
        var online=accounts.Active!;Check(online.ProfileId==profile.Id&&online.ProviderId==authority.ProviderId,"selected exact authority profile");
        var file=Path.Combine(root,"accounts","private",online.AccountId.ToString("N")+".bin");
        var encrypted=await File.ReadAllBytesAsync(file);Check(!Encoding.UTF8.GetString(encrypted).Contains("fixture-token"),"OS encrypted token at rest");
        var catalog=await File.ReadAllTextAsync(Path.Combine(root,"accounts","accounts.json"));
        Check(!catalog.Contains("fixture-token")&&!catalog.Contains("fixture-password")&&!catalog.Contains("fixture-user"),"catalog excludes credentials and login identifier");
        identity=await accounts.GetAsync(default);Check(identity.AccessToken==fake.Token&&identity.ProfileId==profile.Id&&identity.Authority==authority.ApiRoot,"validate and normalized third-party identity");
        Check(!identity.ToString().Contains(fake.Token)&&!JsonSerializer.Serialize(identity).Contains(fake.Token),"identity diagnostics safe");
        fake.ValidateStatus=403;identity=await accounts.GetAsync(default);Check(fake.Refreshes>=2&&identity.AccessToken==fake.Token,"expired validation refreshes session");
        fake.WrongSelected=true;await Error(AccountError.InvalidMetadata,()=>accounts.GetAsync(default));fake.WrongSelected=false;
        fake.RefreshStatus=403;await Error(AccountError.Expired,()=>accounts.GetAsync(default));fake.RefreshStatus=200;
        fake.ValidateStatus=503;await Error(AccountError.Unavailable,()=>accounts.GetAsync(default));fake.ValidateStatus=204;
        fake.NetworkFailure=true;await Error(AccountError.Network,()=>accounts.GetAsync(default));fake.NetworkFailure=false;
        Check(accounts.Active!.AccountId==online.AccountId,"temporary outage preserves selected account");
        var copyId=Guid.NewGuid();File.Copy(file,Path.Combine(root,"accounts","private",copyId.ToString("N")+".bin"));
        await Error(AccountError.Storage,()=>credentials.ReadAsync(copyId,default));
        var corrupt=(byte[])encrypted.Clone();corrupt[^1]^=0x80;await File.WriteAllBytesAsync(file,corrupt);await Error(AccountError.Storage,()=>credentials.ReadAsync(online.AccountId,default));
        await File.WriteAllBytesAsync(file,encrypted);
        await accounts.LogoutAsync(online.AccountId,false,default);Check(accounts.Active is null&&await credentials.ReadAsync(online.AccountId,default) is null&&fake.Invalidates==1,"logout invalidates and clears local credentials");
        await Error(AccountError.Expired,()=>accounts.ActivateAsync(online.AccountId,default));
        await accounts.LogoutAsync(online.AccountId,true,default);Check(accounts.Accounts.Count==1,"remove online account");
        await accounts.ActivateAsync(first.AccountId,default);Check(accounts.Active!.ProfileId==first.ProfileId,"switch back offline");
        fake.BadClient=true;await Error(AccountError.InvalidMetadata,()=>accounts.LoginAsync(MinecraftAccountType.Yggdrasil,authority,"u","p",default));fake.BadClient=false;
        fake.Malformed=true;await Error(AccountError.InvalidMetadata,()=>accounts.LoginAsync(MinecraftAccountType.Yggdrasil,authority,"u","p",default));fake.Malformed=false;
        fake.CrossOrigin=true;await Error(AccountError.InvalidMetadata,()=>accounts.AddAuthorityAsync("https://auth.example.org/",default));fake.CrossOrigin=false;
        foreach(var url in new[]{"http://auth.example.org/","https://user:pass@auth.example.org/","https://localhost/","https://127.0.0.1/","https://192.168.1.1/","https://[::1]/","https://auth.example.org/?key=value","https://auth.example.org/#fragment"})
        {try{AuthenticationUrlPolicy.Parse(url);throw new Exception("Unsafe URL accepted");}catch(AccountException){_checks++;}}
        foreach(var ip in new[]{"10.0.0.1","172.16.0.1","169.254.169.254","100.64.0.1","198.18.0.1","::ffff:127.0.0.1","2001:db8::1","2002:7f00:1::"})Check(!AuthenticationUrlPolicy.IsPublic(IPAddress.Parse(ip)),"private/special IP denied");
        Check(AuthenticationUrlPolicy.IsPublic(IPAddress.Parse("1.1.1.1"))&&AuthenticationUrlPolicy.IsPublic(IPAddress.Parse("2606:4700:4700::1111")),"public IP permitted");
        using(var transport=new AuthenticationTransport())await Error(AccountError.InvalidInput,()=>transport.SendAsync(new("https://127.0.0.1/"),"GET",null,default));
        var option=new MLaunchOption();await AccountLaunchMapping.ApplyAsync(option,new("Steve",first.ProfileId),null,default);Check(option.Session!.UUID==first.ProfileId.ToString("N")&&!option.ExtraJvmArguments.SelectMany(a=>a.Values).Any(a=>a.Contains("javaagent")),"offline launch mapping has no agent");
        await Error(AccountError.Unavailable,()=>AccountLaunchMapping.ApplyAsync(new(),identity,null,default));
        var snapshot=await File.ReadAllBytesAsync(Path.Combine(root,"accounts","accounts.json"));
        await File.WriteAllTextAsync(Path.Combine(root,"accounts","accounts.json"),"{broken");
        await Error(AccountError.Storage,()=>store.LoadAsync(default));await Error(AccountError.Storage,()=>store.SaveAsync(new([],[],null),default));
        Check(await File.ReadAllTextAsync(Path.Combine(root,"accounts","accounts.json"))=="{broken","corrupt catalog preserved");
        await File.WriteAllBytesAsync(Path.Combine(root,"accounts","accounts.json"),snapshot);
        using(var cancel=new CancellationTokenSource()){cancel.Cancel();try{await accounts.GetAsync(cancel.Token);throw new Exception("Cancellation ignored");}catch(OperationCanceledException){_checks++;}}
        accounts.Changed+=()=>throw new Exception("Observer fixture");await accounts.ActivateAsync(first.AccountId,default);Check(accounts.Active is not null,"observer error cannot corrupt transaction");
        Console.WriteLine($"PASS: {_checks} account, protocol, DPAPI, SSRF, restore, profile and launch mapping checks.");
    }
    private static void Check(bool value,string label){if(!value)throw new Exception(label);_checks++;}
    private static async Task Error(AccountError expected,Func<Task> action){try{await action();}catch(AccountException e) when(e.Code==expected){_checks++;return;}throw new Exception("Expected safe account error: "+expected);}
    private sealed class Protocol:IAuthenticationTransport
    {
        internal bool ManyProfiles,BadClient,Malformed,WrongSelected,NetworkFailure,CrossOrigin;
        internal int ValidateStatus=204,RefreshStatus=200,Refreshes,Invalidates;
        internal string Token="fixture-token-1";
        private readonly AccountProfile[] _profiles=[new("FixtureOne",Guid.Parse("11111111-1111-4111-8111-111111111111")),new("FixtureTwo",Guid.Parse("22222222-2222-4222-8222-222222222222"))];
        public Task<AuthenticationResponse> SendAsync(Uri uri,string method,ReadOnlyMemory<byte>? body,CancellationToken token)
        {
            token.ThrowIfCancellationRequested();if(NetworkFailure)throw new AccountException(AccountError.Network,"本地协议测试网络不可用。");
            if(method=="GET")return Task.FromResult(Response(200,new{meta=new{serverName="Local protocol fixture"},skinDomains=Array.Empty<string>()},CrossOrigin?"https://other.example.org/api/":null));
            using var data=JsonDocument.Parse(body!.Value);var payload=data.RootElement;var endpoint=uri.Segments.Last();
            if(endpoint=="validate")return Task.FromResult(new AuthenticationResponse(ValidateStatus,[]));
            if(endpoint=="invalidate"){Invalidates++;return Task.FromResult(new AuthenticationResponse(204,[]));}
            if(endpoint=="refresh"){Refreshes++;if(RefreshStatus!=200)return Task.FromResult(new AuthenticationResponse(RefreshStatus,[]));Token="fixture-token-"+(Refreshes+1);}
            if(Malformed)return Task.FromResult(new AuthenticationResponse(200,Encoding.UTF8.GetBytes("[]")));
            var profiles=ManyProfiles?_profiles:[_profiles[0]];AccountProfile? selected=ManyProfiles&&endpoint=="authenticate"?null:profiles[0];
            if(payload.TryGetProperty("selectedProfile",out var choice))selected=profiles.Single(p=>p.Id.ToString("N")==choice.GetProperty("id").GetString());
            if(WrongSelected)selected=_profiles[0];
            object Map(AccountProfile p)=>new{id=p.Id.ToString("N"),name=p.Name};
            return Task.FromResult(Response(200,new{accessToken=Token,clientToken=BadClient?"bad":payload.GetProperty("clientToken").GetString(),availableProfiles=profiles.Select(Map),selectedProfile=selected is null?null:Map(selected)}));
        }
        private static AuthenticationResponse Response(int status,object body,string? location=null)=>new(status,JsonSerializer.SerializeToUtf8Bytes(body),location);
    }
}
