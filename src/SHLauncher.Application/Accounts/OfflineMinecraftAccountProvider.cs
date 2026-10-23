using SHLauncher.Contracts.Accounts;
namespace SHLauncher.Application.Accounts;
public sealed class OfflineMinecraftAccountProvider:IMinecraftAccountProvider
{
    public MinecraftAccountType Type=>MinecraftAccountType.Offline;
    public Task<ProviderLoginResult> LoginAsync(AuthenticationAuthority? authority,string identifier,string password,CancellationToken token)
    {token.ThrowIfCancellationRequested();OfflineIdentity.ValidateName(identifier);var profile=new AccountProfile(identifier,OfflineIdentity.Uuid(identifier));return Task.FromResult(new ProviderLoginResult([profile],[],profile));}
    public Task<ProviderLoginResult> RestoreAsync(MinecraftAccount account,AuthenticationAuthority? authority,ReadOnlyMemory<byte> credential,CancellationToken token)
    {
        if(account.ProfileId!=OfflineIdentity.Uuid(account.ProfileName))throw new AccountException(AccountError.Storage,"离线身份记录不一致，文件已保留，请检查账号记录。");
        return LoginAsync(null,account.ProfileName,"",token);
    }
    public Task<ProviderLoginResult> SelectProfileAsync(AuthenticationAuthority? authority,ProviderLoginResult login,AccountProfile profile,CancellationToken token)=>LoginAsync(null,profile.Name,"",token);
    public Task InvalidateAsync(AuthenticationAuthority? authority,ReadOnlyMemory<byte> credential,CancellationToken token)=>Task.CompletedTask;
    public LaunchIdentity Identity(MinecraftAccount account,AuthenticationAuthority? authority,ReadOnlyMemory<byte> credential)=>new(account.ProfileName,account.ProfileId);
}
public sealed class MicrosoftAccountProvider:IMinecraftAccountProvider
{
    public MinecraftAccountType Type=>MinecraftAccountType.Microsoft;
    private static AccountException Unavailable()=>new(AccountError.Unavailable,"Microsoft 正版登录即将支持，目前尚未配置 SHLN 自有应用授权。");
    public Task<ProviderLoginResult> LoginAsync(AuthenticationAuthority? authority,string identifier,string password,CancellationToken token)=>throw Unavailable();
    public Task<ProviderLoginResult> RestoreAsync(MinecraftAccount account,AuthenticationAuthority? authority,ReadOnlyMemory<byte> credential,CancellationToken token)=>throw Unavailable();
    public Task<ProviderLoginResult> SelectProfileAsync(AuthenticationAuthority? authority,ProviderLoginResult login,AccountProfile profile,CancellationToken token)=>throw Unavailable();
    public Task InvalidateAsync(AuthenticationAuthority? authority,ReadOnlyMemory<byte> credential,CancellationToken token)=>Task.CompletedTask;
    public LaunchIdentity Identity(MinecraftAccount account,AuthenticationAuthority? authority,ReadOnlyMemory<byte> credential)=>throw Unavailable();
}
