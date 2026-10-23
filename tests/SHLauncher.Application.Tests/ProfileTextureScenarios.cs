using System.Text.Json;
using SHLauncher.Contracts.Accounts;
using SHLauncher.Infrastructure.Accounts;
internal static class ProfileTextureScenarios
{
    internal static async Task Run()
    {
        var root=new DirectoryInfo(AppContext.BaseDirectory);while(root is not null&&!File.Exists(Path.Combine(root.FullName,"SHLauncherNext.sln")))root=root.Parent;
        var workspace=Path.Combine(root!.FullName,".devdata","texture-tests",Guid.NewGuid().ToString("N"));
        var png=await File.ReadAllBytesAsync(Path.Combine(root.FullName,"src","SHLauncher.UI","Assets","Skins","shln-default.png"));
        void Check(bool valid,string message){if(!valid)throw new Exception(message);}
        Check(ProfileTextureService.ValidPng(png,true),"bundled default PNG valid");
        var corrupt=(byte[])png.Clone();corrupt[corrupt.Length-1]^=1;Check(!ProfileTextureService.ValidPng(corrupt,true),"CRC corruption rejected");
        Check(!ProfileTextureService.ValidPng(png[..32],true),"truncated PNG rejected");
        var account=new MinecraftAccount(Guid.NewGuid(),MinecraftAccountType.Yggdrasil,"Fixture",Guid.NewGuid(),"fixture","Fixture",AccountAuthenticationState.Ready,DateTimeOffset.UtcNow);
        var authority=new AuthenticationAuthority("fixture","Fixture",new Uri("https://auth.example/"),false,DateTimeOffset.UtcNow);
        var transport=new Fixture(account.ProfileId,png);var service=new ProfileTextureService(transport,workspace);
        var first=await service.ReadAsync(account,authority,false,default);Check(first.Skin!.SequenceEqual(png)&&!first.FromCache,"public profile skin resolves");
        var requests=transport.Requests;var cached=await service.ReadAsync(account,authority,false,default);Check(cached.FromCache&&requests==transport.Requests,"fresh cache avoids network");
        transport.Fail=true;var fallback=await service.ReadAsync(account,authority,true,default);Check(fallback.FromCache&&fallback.Skin!.SequenceEqual(png),"failed refresh preserves working cached skin");
        transport.Fail=false;transport.Remove=true;
        var removed=await service.ReadAsync(account,authority,true,default);Check(removed.Skin is null,"provider removal clears active texture");
        transport.Fail=true;var reopened=await new ProfileTextureService(transport,workspace).ReadAsync(account,authority,false,default);Check(reopened.FromCache&&reopened.Skin is null,"removed skin remains absent after service restart");
        transport.Remove=false;
        var offline=await service.ReadAsync(account with{AccountType=MinecraftAccountType.Offline},null,false,default);Check(offline.Skin is null&&!offline.Capabilities.CanReadSkin,"offline uses bundled default without provider request");
        transport.Fail=false;transport.ProfileId=Guid.NewGuid();var mismatch=await new ProfileTextureService(transport,workspace+"-mismatch").ReadAsync(account,authority,true,default);Check(mismatch.Skin is null,"wrong profile response never becomes avatar");
        Console.WriteLine("PASS: profile textures identity, bounded PNG CRC, offline fallback, cache hit and failed-refresh preservation.");
    }
    private sealed class Fixture(Guid id,byte[] png):IAuthenticationTransport
    {
        public Guid ProfileId=id;public bool Fail;public bool Remove;public int Requests;
        public Task<AuthenticationResponse> SendAsync(Uri uri,string method,ReadOnlyMemory<byte>? body,CancellationToken token)
        {
            Requests++;if(Fail)throw new AccountException(AccountError.Network,"fixture failure");
            if(uri.Host=="textures.example")return Task.FromResult(new AuthenticationResponse(200,png));
            if(Remove)return Task.FromResult(new AuthenticationResponse(200,JsonSerializer.SerializeToUtf8Bytes(new{id=ProfileId.ToString("N"),properties=Array.Empty<object>()})));
            var value=Convert.ToBase64String(JsonSerializer.SerializeToUtf8Bytes(new{profileId=ProfileId.ToString("N"),textures=new{SKIN=new{url="https://textures.example/skin.png"}}}));
            return Task.FromResult(new AuthenticationResponse(200,JsonSerializer.SerializeToUtf8Bytes(new{id=ProfileId.ToString("N"),properties=new[]{new{name="textures",value}}})));
        }
    }
}
