using System.Security.Cryptography;
using System.Text.Json;
using SHLauncher.Contracts.Accounts;
namespace SHLauncher.Infrastructure.Accounts;
public sealed class YggdrasilAccountProvider(IAuthenticationTransport transport):IMinecraftAccountProvider,IAuthorityDiscovery
{
    public MinecraftAccountType Type=>MinecraftAccountType.Yggdrasil;
    public async Task<AuthenticationAuthority> DiscoverAsync(string url,CancellationToken token)
    {
        var root=ParseRoot(url);var response=await transport.SendAsync(root,"GET",null,token).ConfigureAwait(false);
        try
        {
            if(response.ApiLocation is not null)
            {
                var next=ParseRoot(new Uri(root,response.ApiLocation).AbsoluteUri);
                if(next.GetLeftPart(UriPartial.Authority)!=root.GetLeftPart(UriPartial.Authority))throw new AccountException(AccountError.InvalidMetadata,"认证服务将 API 指向另一个域名，请直接添加并核实该 API 地址。");
                CryptographicOperations.ZeroMemory(response.Body);root=next;response=await transport.SendAsync(root,"GET",null,token).ConfigureAwait(false);
            }
            if(response.StatusCode!=200)throw new AccountException(AccountError.Unavailable,"无法读取认证服务器信息，请确认 API 地址。");
            using var json=Parse(response.Body);var data=json.RootElement;
            if(data.ValueKind!=JsonValueKind.Object||!data.TryGetProperty("meta",out var meta)||meta.ValueKind!=JsonValueKind.Object)throw new AccountException(AccountError.InvalidMetadata,"认证服务器信息格式无效。");
            var name=meta.TryGetProperty("serverName",out var title)&&title.ValueKind==JsonValueKind.String?title.GetString():root.Host;
            if(string.IsNullOrWhiteSpace(name)||name.Length>128||name.Any(char.IsControl))throw new AccountException(AccountError.InvalidMetadata,"认证服务器名称无效。");
            if(data.TryGetProperty("signaturePublickey",out var key))
            {if(key.ValueKind!=JsonValueKind.String)throw new AccountException(AccountError.InvalidMetadata,"认证服务器公钥无效。");try{using var rsa=RSA.Create();rsa.ImportFromPem(key.GetString()!);}catch(Exception e) when(e is ArgumentException or CryptographicException){throw new AccountException(AccountError.InvalidMetadata,"认证服务器公钥无效。");}}
            if(data.TryGetProperty("skinDomains",out var domains)&&(domains.ValueKind!=JsonValueKind.Array||domains.GetArrayLength()>100||domains.EnumerateArray().Any(d=>d.ValueKind!=JsonValueKind.String||d.GetString()!.Length>255)))throw new AccountException(AccountError.InvalidMetadata,"认证服务器皮肤域信息无效。");
            var email=meta.TryGetProperty("feature.non_email_login",out var nonEmail)&&nonEmail.ValueKind==JsonValueKind.False;
            return new(Guid.NewGuid().ToString("N"),name,root,email,DateTimeOffset.UtcNow);
        }
        finally{CryptographicOperations.ZeroMemory(response.Body);}
    }
    public async Task<ProviderLoginResult> LoginAsync(AuthenticationAuthority? authority,string identifier,string password,CancellationToken token)
    {
        if(authority is null||string.IsNullOrWhiteSpace(identifier)||identifier.Length>256||password.Length is 0 or >4096)throw new AccountException(AccountError.InvalidInput,"请填写认证服务、账号和密码。");
        var client=Guid.NewGuid().ToString("N");
        return await Authenticate(authority,"authenticate",new{agent=new{name="Minecraft",version=1},username=identifier,password,clientToken=client,requestUser=true},client,token).ConfigureAwait(false);
    }
    public async Task<ProviderLoginResult> RestoreAsync(MinecraftAccount account,AuthenticationAuthority? authority,ReadOnlyMemory<byte> credential,CancellationToken token)
    {
        var session=ReadSession(authority,credential);var response=await Post(authority!,"validate",new{accessToken=session.AccessToken,clientToken=session.ClientToken},token).ConfigureAwait(false);
        CryptographicOperations.ZeroMemory(response.Body);
        if(response.StatusCode==204){var profile=new AccountProfile(account.ProfileName,account.ProfileId);return new([profile],credential.ToArray(),profile);}
        if(response.StatusCode is not (401 or 403))throw Status(response.StatusCode);
        return await Authenticate(authority!,"refresh",new{accessToken=session.AccessToken,clientToken=session.ClientToken,selectedProfile=new{id=account.ProfileId.ToString("N"),name=account.ProfileName},requestUser=true},session.ClientToken,token).ConfigureAwait(false);
    }
    public Task<ProviderLoginResult> SelectProfileAsync(AuthenticationAuthority? authority,ProviderLoginResult login,AccountProfile profile,CancellationToken token)
    {
        if(!login.Profiles.Contains(profile))throw new AccountException(AccountError.InvalidInput,"角色不属于本次认证结果。");
        if(login.Selected==profile)return Task.FromResult(new ProviderLoginResult(login.Profiles,login.Credential.ToArray(),profile));
        var session=ReadSession(authority,login.Credential);
        return Authenticate(authority!,"refresh",new{accessToken=session.AccessToken,clientToken=session.ClientToken,selectedProfile=new{id=profile.Id.ToString("N"),name=profile.Name},requestUser=true},session.ClientToken,token);
    }
    public async Task InvalidateAsync(AuthenticationAuthority? authority,ReadOnlyMemory<byte> credential,CancellationToken token)
    {
        var session=ReadSession(authority,credential);var response=await Post(authority!,"invalidate",new{accessToken=session.AccessToken,clientToken=session.ClientToken},token).ConfigureAwait(false);
        CryptographicOperations.ZeroMemory(response.Body);if(response.StatusCode is not (204 or 401 or 403))throw Status(response.StatusCode);
    }
    public LaunchIdentity Identity(MinecraftAccount account,AuthenticationAuthority? authority,ReadOnlyMemory<byte> credential)
    {var session=ReadSession(authority,credential);return new(account.ProfileName,account.ProfileId,session.AccessToken,"mojang","{}",authority!.ApiRoot);}
    private async Task<ProviderLoginResult> Authenticate(AuthenticationAuthority authority,string endpoint,object payload,string expectedClient,CancellationToken token)
    {
        var response=await Post(authority,endpoint,payload,token).ConfigureAwait(false);
        try
        {
            if(response.StatusCode!=200)throw Status(response.StatusCode);
            using var json=Parse(response.Body);var data=json.RootElement;
            var access=Text(data,"accessToken",8192);var client=Text(data,"clientToken",128);
            if(client!=expectedClient)throw new AccountException(AccountError.InvalidMetadata,"认证服务器返回的会话标识不匹配。");
            var profiles=data.TryGetProperty("availableProfiles",out var list)&&list.ValueKind==JsonValueKind.Array?list.EnumerateArray().Select(Profile).ToArray():[];
            if(profiles.Length>100)throw new AccountException(AccountError.InvalidMetadata,"认证服务器返回的角色过多。");
            var selected=data.TryGetProperty("selectedProfile",out var item)&&item.ValueKind==JsonValueKind.Object?Profile(item):null;
            if(profiles.Length==0&&selected is not null)profiles=[selected];
            if(selected is not null&&!profiles.Contains(selected))throw new AccountException(AccountError.InvalidMetadata,"认证服务器角色选择不一致。");
            var credential=JsonSerializer.SerializeToUtf8Bytes(new Session(authority.ApiRoot.AbsoluteUri,access,client));
            return new(profiles,credential,selected);
        }
        finally{CryptographicOperations.ZeroMemory(response.Body);}
    }
    private async Task<AuthenticationResponse> Post(AuthenticationAuthority authority,string endpoint,object payload,CancellationToken token)
    {
        var root=ParseRoot(authority.ApiRoot.AbsoluteUri);var bytes=JsonSerializer.SerializeToUtf8Bytes(payload);
        try{return await transport.SendAsync(new Uri(root,"authserver/"+endpoint),"POST",bytes,token).ConfigureAwait(false);}
        finally{CryptographicOperations.ZeroMemory(bytes);}
    }
    private static Session ReadSession(AuthenticationAuthority? authority,ReadOnlyMemory<byte> credential)
    {
        try
        {
            var value=JsonSerializer.Deserialize<Session>(credential.Span);
            if(authority is null||value is null||value.ApiRoot!=authority.ApiRoot.AbsoluteUri||string.IsNullOrWhiteSpace(value.AccessToken)||value.AccessToken.Length>8192||string.IsNullOrWhiteSpace(value.ClientToken)||value.ClientToken.Length>128)throw new JsonException();
            return value;
        }
        catch(JsonException){throw new AccountException(AccountError.Expired,"登录状态已过期或不属于此认证服务，请重新登录。");}
    }
    private static JsonDocument Parse(byte[] bytes)
    {try{return JsonDocument.Parse(bytes,new(){MaxDepth=16});}catch(JsonException){throw new AccountException(AccountError.InvalidMetadata,"认证服务器响应格式无效。");}}
    private static string Text(JsonElement data,string name,int max)
    {if(data.ValueKind!=JsonValueKind.Object||!data.TryGetProperty(name,out var item)||item.ValueKind!=JsonValueKind.String||string.IsNullOrWhiteSpace(item.GetString())||item.GetString()!.Length>max||item.GetString()!.Any(char.IsControl))throw new AccountException(AccountError.InvalidMetadata,"认证服务器响应缺少有效身份信息。");return item.GetString()!;}
    private static AccountProfile Profile(JsonElement data)
    {var name=Text(data,"name",64);if(!Guid.TryParseExact(Text(data,"id",32),"N",out var id)||id==Guid.Empty)throw new AccountException(AccountError.InvalidMetadata,"认证服务器返回的角色 UUID 无效。");return new(name,id);}
    private static AccountException Status(int status)=>status switch{401 or 403=>new(AccountError.Expired,"账号或密码不正确，或登录状态已过期，请重新登录。"),>=500 or 429=>new(AccountError.Unavailable,"认证服务器暂时不可用，请稍后重试。"),_=>new(AccountError.InvalidMetadata,"认证服务器未接受该请求，请检查服务兼容性。")};
    // Syntax only here; the production transport independently resolves and pins public IPs at every actual connection.
    private static Uri ParseRoot(string value)
    {if(!Uri.TryCreate(value,UriKind.Absolute,out var uri)||uri.Scheme!="https"||uri.UserInfo.Length!=0||uri.Query.Length!=0||uri.Fragment.Length!=0||value.Length>2048)throw new AccountException(AccountError.InvalidInput,"认证服务器地址无效，请使用 HTTPS API 地址。");return new(uri.AbsoluteUri.TrimEnd('/')+"/");}
    private sealed record Session(string ApiRoot,string AccessToken,string ClientToken){public override string ToString()=>"Authentication session [credentials hidden]";}
}
