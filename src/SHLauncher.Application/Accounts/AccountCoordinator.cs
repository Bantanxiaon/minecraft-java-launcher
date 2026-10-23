using System.Security.Cryptography;
using SHLauncher.Contracts.Accounts;
namespace SHLauncher.Application.Accounts;
public sealed class AccountCoordinator(IAccountCatalogStore store,ICredentialStore credentials,IEnumerable<IMinecraftAccountProvider> providers,
    IAuthorityDiscovery discovery,Func<bool> canChange):IAccountProvider,IDisposable
{
    private readonly SemaphoreSlim _serial=new(1);
    private readonly IReadOnlyDictionary<MinecraftAccountType,IMinecraftAccountProvider> _providers=providers.ToDictionary(p=>p.Type);
    private AccountCatalog _catalog=new([],[],null);
    private ProviderLoginResult? _pending;
    private AuthenticationAuthority? _pendingAuthority;
    private MinecraftAccountType _pendingType;
    private bool _initialized;
    public event Action? Changed;
    public IReadOnlyList<MinecraftAccount> Accounts=>_catalog.Accounts;
    public IReadOnlyList<AuthenticationAuthority> Authorities=>_catalog.Authorities;
    public MinecraftAccount? Active=>Accounts.SingleOrDefault(a=>a.AccountId==_catalog.ActiveAccountId);
    public IReadOnlyList<AccountProfile> PendingProfiles=>_pending?.Profiles??[];
    public bool CanUseOnline=>credentials.IsSupported;
    public async Task<bool> InitializeAsync(CancellationToken token)
    {
        await _serial.WaitAsync(token).ConfigureAwait(false);
        try
        {
            if(!_initialized){_catalog=await store.LoadAsync(token).ConfigureAwait(false);_initialized=true;}
            if(Active is null)return false;
            await RestoreCoreAsync(Active,token).ConfigureAwait(false);return true;
        }
        finally{_serial.Release();Notify();}
    }
    public async Task<AuthenticationAuthority> AddAuthorityAsync(string url,CancellationToken token)
    {
        var authority=await discovery.DiscoverAsync(url,token).ConfigureAwait(false);
        await _serial.WaitAsync(token).ConfigureAwait(false);
        try
        {
            Ready();var existing=Authorities.FirstOrDefault(a=>a.ApiRoot==authority.ApiRoot);
            authority=authority with{ProviderId=existing?.ProviderId??authority.ProviderId};
            await PersistAsync(_catalog with{Authorities=Authorities.Where(a=>a.ProviderId!=authority.ProviderId).Append(authority).ToArray()},token).ConfigureAwait(false);
            return authority;
        }
        finally{_serial.Release();Notify();}
    }
    public async Task<bool> LoginAsync(MinecraftAccountType type,AuthenticationAuthority? authority,string identifier,string password,CancellationToken token)
    {
        await _serial.WaitAsync(token).ConfigureAwait(false);
        try
        {
            Ready();EnsureChange();CancelPending();
            if(type==MinecraftAccountType.Yggdrasil&&!credentials.IsSupported)throw new AccountException(AccountError.Unavailable,"当前平台尚无可用的安全凭据存储，第三方登录暂不可用。");
            if(type==MinecraftAccountType.Yggdrasil&&(authority is null||!Authorities.Contains(authority)))throw new AccountException(AccountError.InvalidInput,"请先选择已添加的认证服务。");
            _pending=await _providers[type].LoginAsync(authority,identifier,password,token).ConfigureAwait(false);
            _pendingAuthority=authority;_pendingType=type;
            if(_pending.Selected is not null||_pending.Profiles.Count==1)
            {await CompletePendingAsync(_pending.Selected??_pending.Profiles[0],token).ConfigureAwait(false);return true;}
            if(_pending.Profiles.Count==0)throw new AccountException(AccountError.InvalidCredentials,"该账号没有可用的 Minecraft 角色。");
            return false;
        }
        catch{CancelPending();throw;}
        finally{_serial.Release();Notify();}
    }
    public async Task SelectProfileAsync(AccountProfile profile,CancellationToken token)
    {
        await _serial.WaitAsync(token).ConfigureAwait(false);
        try{Ready();EnsureChange();if(_pending is null||!_pending.Profiles.Contains(profile))throw new AccountException(AccountError.InvalidInput,"请选择可用角色。");await CompletePendingAsync(profile,token).ConfigureAwait(false);}
        finally{_serial.Release();Notify();}
    }
    private async Task CompletePendingAsync(AccountProfile profile,CancellationToken token)
    {
        using var selected=await _providers[_pendingType].SelectProfileAsync(_pendingAuthority,_pending!,profile,token).ConfigureAwait(false);
        if(selected.Selected!=profile)throw new AccountException(AccountError.InvalidMetadata,"认证服务器未确认所选角色，请重新登录。");
        var providerId=_pendingAuthority?.ProviderId??_pendingType.ToString();
        var old=Accounts.SingleOrDefault(a=>a.ProfileId==profile.Id&&a.ProviderId==providerId);
        var account=new MinecraftAccount(old?.AccountId??Guid.NewGuid(),_pendingType,profile.Name,profile.Id,providerId,
            _pendingAuthority?.DisplayName??"离线游戏",AccountAuthenticationState.Ready,old?.CreatedAt??DateTimeOffset.UtcNow,DateTimeOffset.UtcNow);
        if(_pendingType!=MinecraftAccountType.Offline)await credentials.WriteAsync(account.AccountId,selected.Credential,token).ConfigureAwait(false);
        await PersistAsync(_catalog with{Accounts=Accounts.Where(a=>a.AccountId!=account.AccountId).Append(account).ToArray(),ActiveAccountId=account.AccountId},token).ConfigureAwait(false);
        CancelPending();
    }
    public async Task ActivateAsync(Guid id,CancellationToken token)
    {
        await _serial.WaitAsync(token).ConfigureAwait(false);
        try{Ready();EnsureChange();var account=Accounts.SingleOrDefault(a=>a.AccountId==id)??throw new AccountException(AccountError.InvalidInput,"找不到该账号。");await RestoreCoreAsync(account,token).ConfigureAwait(false);await PersistAsync(_catalog with{ActiveAccountId=id},token).ConfigureAwait(false);CancelPending();}
        finally{_serial.Release();Notify();}
    }
    public async Task LogoutAsync(Guid id,bool remove,CancellationToken token)
    {
        await _serial.WaitAsync(token).ConfigureAwait(false);
        try
        {
            Ready();EnsureChange();var account=Accounts.SingleOrDefault(a=>a.AccountId==id)??throw new AccountException(AccountError.InvalidInput,"找不到该账号。");
            var secret=account.AccountType==MinecraftAccountType.Offline?null:await credentials.ReadAsync(id,token).ConfigureAwait(false);
            try{if(secret is not null)try{await _providers[account.AccountType].InvalidateAsync(Authority(account),secret,token).ConfigureAwait(false);}catch(AccountException){/* Local logout remains possible when remote invalidation is unavailable. */}}
            finally{if(secret is not null)CryptographicOperations.ZeroMemory(secret);}
            await PersistAsync(_catalog with{Accounts=remove?Accounts.Where(a=>a.AccountId!=id).ToArray():Accounts.Select(a=>a.AccountId==id?a with{AuthenticationState=account.AccountType==MinecraftAccountType.Offline?AccountAuthenticationState.Ready:AccountAuthenticationState.Expired}:a).ToArray(),ActiveAccountId=_catalog.ActiveAccountId==id?null:_catalog.ActiveAccountId},token).ConfigureAwait(false);
            if(account.AccountType!=MinecraftAccountType.Offline)await credentials.RemoveAsync(id,CancellationToken.None).ConfigureAwait(false);
            CancelPending();
        }
        finally{_serial.Release();Notify();}
    }
    public async Task<LaunchIdentity> GetAsync(CancellationToken token)
    {
        await _serial.WaitAsync(token).ConfigureAwait(false);
        try{Ready();var account=Active??throw new AccountException(AccountError.Expired,"请选择一个 Minecraft 账号。");return await RestoreCoreAsync(account,token).ConfigureAwait(false);}
        finally{_serial.Release();Notify();}
    }
    private async Task<LaunchIdentity> RestoreCoreAsync(MinecraftAccount account,CancellationToken token)
    {
        var bytes=account.AccountType==MinecraftAccountType.Offline?[]:await credentials.ReadAsync(account.AccountId,token).ConfigureAwait(false)??throw new AccountException(AccountError.Expired,"登录状态已过期，请重新登录。");
        try
        {
            using var restored=await _providers[account.AccountType].RestoreAsync(account,Authority(account),bytes,token).ConfigureAwait(false);
            if(restored.Selected is null||restored.Selected.Id!=account.ProfileId||restored.Selected.Name!=account.ProfileName)throw new AccountException(AccountError.InvalidMetadata,"认证服务器返回了不同角色，请重新登录选择。");
            if(account.AccountType!=MinecraftAccountType.Offline)await credentials.WriteAsync(account.AccountId,restored.Credential,token).ConfigureAwait(false);
            var updated=account with{AuthenticationState=AccountAuthenticationState.Ready,LastUsedAt=DateTimeOffset.UtcNow};
            await PersistAsync(_catalog with{Accounts=Accounts.Select(a=>a.AccountId==account.AccountId?updated:a).ToArray()},token).ConfigureAwait(false);
            return _providers[account.AccountType].Identity(updated,Authority(account),restored.Credential);
        }
        finally{CryptographicOperations.ZeroMemory(bytes);}
    }
    private AuthenticationAuthority? Authority(MinecraftAccount account)=>Authorities.SingleOrDefault(a=>a.ProviderId==account.ProviderId);
    private void Ready(){if(!_initialized)throw new AccountException(AccountError.Storage,"账号记录尚未加载完成，请重试。");}
    private void EnsureChange(){if(!canChange())throw new AccountException(AccountError.Busy,"请先结束当前游戏，再更换或移除账号。");}
    private async Task PersistAsync(AccountCatalog value,CancellationToken token){await store.SaveAsync(value,token).ConfigureAwait(false);_catalog=value;}
    private void CancelPending(){_pending?.Dispose();_pending=null;_pendingAuthority=null;}
    private void Notify(){if(Changed is { } changed)foreach(Action observer in changed.GetInvocationList())try{observer();}catch{/* Observers cannot roll back secure storage operations. */}}
    public void Dispose(){CancelPending();_serial.Dispose();}
}
