namespace SHLauncher.Contracts.Accounts;
public enum MinecraftAccountType { Offline,Yggdrasil,Microsoft }
public enum AccountAuthenticationState { Ready,Expired,Unavailable }
public enum AccountError { InvalidInput,InvalidCredentials,Expired,Network,Unavailable,InvalidMetadata,Storage,Busy }
public sealed class AccountException(AccountError code,string message):Exception(message){public AccountError Code{get;}=code;}
public sealed record MinecraftAccount(Guid AccountId,MinecraftAccountType AccountType,string ProfileName,Guid ProfileId,string ProviderId,
    string ProviderDisplayName,AccountAuthenticationState AuthenticationState,DateTimeOffset CreatedAt,DateTimeOffset? LastUsedAt=null);
public sealed record AuthenticationAuthority(string ProviderId,string DisplayName,Uri ApiRoot,bool EmailOnly,DateTimeOffset LastVerifiedAt);
public sealed record AccountProfile(string Name,Guid Id);
public sealed record AccountCatalog(IReadOnlyList<MinecraftAccount> Accounts,IReadOnlyList<AuthenticationAuthority> Authorities,Guid? ActiveAccountId,int SchemaVersion=1);
public sealed class ProviderLoginResult(IReadOnlyList<AccountProfile> profiles,byte[] credential,AccountProfile? selected=null):IDisposable
{
    public IReadOnlyList<AccountProfile> Profiles{get;}=profiles;
    public AccountProfile? Selected{get;}=selected;
    [System.Text.Json.Serialization.JsonIgnore] public byte[] Credential{get;}=credential;
    public void Dispose()=>System.Security.Cryptography.CryptographicOperations.ZeroMemory(Credential);
    public override string ToString()=>"Provider result [credentials hidden]";
}
public interface IMinecraftAccountProvider
{
    MinecraftAccountType Type{get;}
    Task<ProviderLoginResult> LoginAsync(AuthenticationAuthority? authority,string identifier,string password,CancellationToken token);
    Task<ProviderLoginResult> RestoreAsync(MinecraftAccount account,AuthenticationAuthority? authority,ReadOnlyMemory<byte> credential,CancellationToken token);
    Task<ProviderLoginResult> SelectProfileAsync(AuthenticationAuthority? authority,ProviderLoginResult login,AccountProfile profile,CancellationToken token);
    Task InvalidateAsync(AuthenticationAuthority? authority,ReadOnlyMemory<byte> credential,CancellationToken token);
    LaunchIdentity Identity(MinecraftAccount account,AuthenticationAuthority? authority,ReadOnlyMemory<byte> credential);
}
public interface IAccountCatalogStore
{
    Task<AccountCatalog> LoadAsync(CancellationToken token);
    Task SaveAsync(AccountCatalog catalog,CancellationToken token);
}
public interface ICredentialProtector
{
    bool IsSupported{get;}
    byte[] Protect(ReadOnlySpan<byte> input);
    byte[] Unprotect(ReadOnlySpan<byte> input);
}
public interface ICredentialStore
{
    bool IsSupported{get;}
    Task<byte[]?> ReadAsync(Guid id,CancellationToken token);
    Task WriteAsync(Guid id,ReadOnlyMemory<byte> credential,CancellationToken token);
    Task RemoveAsync(Guid id,CancellationToken token);
}
public interface IAuthorityDiscovery { Task<AuthenticationAuthority> DiscoverAsync(string url,CancellationToken token); }
public sealed class AuthenticationResponse(int statusCode,byte[] body,string? apiLocation=null)
{
    public int StatusCode{get;}=statusCode;
    [System.Text.Json.Serialization.JsonIgnore] public byte[] Body{get;}=body;
    public string? ApiLocation{get;}=apiLocation;
    public override string ToString()=>"Authentication response [body hidden]";
}
public interface IAuthenticationTransport
{
    Task<AuthenticationResponse> SendAsync(Uri uri,string method,ReadOnlyMemory<byte>? body,CancellationToken token);
}
public sealed record AuthenticationAgent(string Path,string Sha256);
public interface IAuthenticationAgentService{Task<AuthenticationAgent> PrepareAsync(CancellationToken token);}
