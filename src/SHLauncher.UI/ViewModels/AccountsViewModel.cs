using Avalonia.Threading;
using SHLauncher.Application.Accounts;
using SHLauncher.Contracts.Accounts;
using SHLauncher.UI.Presentation;
namespace SHLauncher.UI.ViewModels;
public sealed class AccountsViewModel:ObservableModel,IAsyncDisposable
{
    private readonly AccountCoordinator? _accounts;
    private readonly CancellationTokenSource _lifetime=new();
    private bool _busy;
    private string _message="",_name="",_identifier="",_password="",_authorityUrl="";
    private AuthenticationAuthority? _authority;
    private MinecraftAccount? _selected;
    private AccountProfile? _profile;
    private string _panel="Choice";
    public event Action? Authenticated;
    public event Action? PanelChanged;
    public bool Busy{get=>_busy;private set{if(Set(ref _busy,value)){Raise(nameof(CanEdit));RefreshCommands();}}}
    public bool CanEdit=>!Busy;
    public string Message{get=>_message;private set=>Set(ref _message,value);}
    public string OfflineName{get=>_name;set=>Set(ref _name,value);}
    public string Identifier{get=>_identifier;set=>Set(ref _identifier,value);}
    public string Password{get=>_password;set=>Set(ref _password,value);}
    public string AuthorityUrl{get=>_authorityUrl;set=>Set(ref _authorityUrl,value);}
    public AuthenticationAuthority? Authority{get=>_authority;set=>Set(ref _authority,value);}
    public MinecraftAccount? SelectedAccount{get=>_selected;set=>Set(ref _selected,value);}
    public AccountProfile? SelectedProfile{get=>_profile;set=>Set(ref _profile,value);}
    public IReadOnlyList<MinecraftAccount> Accounts=>_accounts?.Accounts??[];
    public IProfileTextureService? ProfileTextures{get;set;}
    public IReadOnlyList<SavedAccountRow> SavedAccounts=>Accounts.Select(a=>new SavedAccountRow(a,selected=>{SelectedAccount=selected;ActivateCommand.Execute(null);},async token=>ProfileTextures is null?null:(await ProfileTextures.ReadAsync(a,Authorities.FirstOrDefault(authority=>authority.ProviderId==a.ProviderId),false,token)).Skin)).ToArray();
    public string Heading=>IsOffline?"离线模式":IsThirdParty?"第三方登录":IsProfiles?"选择角色":"登录";
    public string Subtitle=>IsChoice?"选择账户":IsOffline?"输入游戏名称":"登录 Minecraft 账户";
    public IReadOnlyList<AuthenticationAuthority> Authorities=>_accounts?.Authorities??[];
    public IReadOnlyList<AccountProfile> Profiles=>_accounts?.PendingProfiles??[];
    public MinecraftAccount? ActiveAccount=>_accounts?.Active;
    public string ActiveName=>_accounts?.Active?.ProfileName??"未选择账号";
    public string ActiveProvider=>_accounts?.Active?.ProviderDisplayName??"添加或选择账号";
    public string ActiveAvatar=>ActiveName[..1].ToUpperInvariant();
    public bool HasActive=>_accounts?.Active is not null;
    public bool HasAccounts=>Accounts.Count>0;
    private bool _canReturn;
    public bool CanReturn{get=>_canReturn&&HasActive;set{_canReturn=value;Raise();}}
    public DelegateCommand? ReturnCommand{get;set;}
    public bool HasMessage=>Message.Length>0;
    public bool IsChoice=>_panel=="Choice";
    public bool IsOffline=>_panel=="Offline";
    public bool IsThirdParty=>_panel=="ThirdParty";
    public bool IsProfiles=>_panel=="Profiles";
    public bool OnlineSupported=>_accounts?.CanUseOnline==true;
    public DelegateCommand OfflineCommand{get;}
    public DelegateCommand ThirdPartyCommand{get;}
    public DelegateCommand BackCommand{get;}
    public AsyncCommand OfflineLoginCommand{get;}
    public AsyncCommand AddAuthorityCommand{get;}
    public AsyncCommand OnlineLoginCommand{get;}
    public AsyncCommand SelectProfileCommand{get;}
    public AsyncCommand ActivateCommand{get;}
    public AsyncCommand LogoutCommand{get;}
    public AsyncCommand RemoveCommand{get;}
    public AccountsViewModel(AccountCoordinator? accounts)
    {
        _accounts=accounts;if(accounts is not null)accounts.Changed+=OnChanged;
        OfflineCommand=new(()=>Show("Offline"),()=>CanEdit);
        ThirdPartyCommand=new(()=>Show("ThirdParty"),()=>CanEdit&&OnlineSupported);
        BackCommand=new(()=>{Password="";Show("Choice");},()=>CanEdit);
        OfflineLoginCommand=Command(async()=>{if(await Required.LoginAsync(MinecraftAccountType.Offline,null,OfflineName,"",_lifetime.Token))Authenticated?.Invoke();});
        AddAuthorityCommand=Command(async()=>{Authority=await Required.AddAuthorityAsync(AuthorityUrl,_lifetime.Token);Message="认证服务已验证并添加。请核实服务名称，再输入账号密码。";});
        OnlineLoginCommand=Command(async()=>{
            var password=Password;Password="";
            if(await Required.LoginAsync(MinecraftAccountType.Yggdrasil,Authority,Identifier,password,_lifetime.Token))Authenticated?.Invoke();else Show("Profiles");
        });
        SelectProfileCommand=Command(async()=>{if(SelectedProfile is null)throw new AccountException(AccountError.InvalidInput,"请选择角色。");await Required.SelectProfileAsync(SelectedProfile,_lifetime.Token);Authenticated?.Invoke();});
        ActivateCommand=Command(async()=>{if(SelectedAccount is null)throw new AccountException(AccountError.InvalidInput,"请选择账号。");await Required.ActivateAsync(SelectedAccount.AccountId,_lifetime.Token);Authenticated?.Invoke();});
        LogoutCommand=Command(()=>Logout(false));RemoveCommand=Command(()=>Logout(true));
    }
    private AccountCoordinator Required=>_accounts??throw new AccountException(AccountError.Unavailable,"当前工作区未接入账号服务。");
    private async Task Logout(bool remove)
    {
        if(SelectedAccount is null)throw new AccountException(AccountError.InvalidInput,"请选择要处理的账号。");
        await Required.LogoutAsync(SelectedAccount.AccountId,remove,_lifetime.Token);SelectedAccount=null;Message=remove?"账号已从本机移除。":"已退出登录。";
    }
    private AsyncCommand Command(Func<Task> action)=>new(async()=>{
        if(Busy)return;Busy=true;Message="";
        try{await action();}
        finally{Busy=false;Refresh();}
    },()=>CanEdit,Error);
    private void Error(Exception error){Password="";Message=error is OperationCanceledException?"操作已取消。":error is AccountException safe?safe.Message:"账号操作未完成，请重试；本地已有记录已保留。";Raise(nameof(HasMessage));}
    private void Show(string panel){_panel=panel;Message="";Refresh();PanelChanged?.Invoke();}
    public void Report(Exception error)=>Error(error);
    private void OnChanged(){if(Dispatcher.UIThread.CheckAccess())Refresh();else Dispatcher.UIThread.Post(Refresh);}
    private void Refresh()
    {
        foreach(var property in new[]{nameof(Accounts),nameof(SavedAccounts),nameof(Heading),nameof(Subtitle),nameof(CanReturn),nameof(Authorities),nameof(Profiles),nameof(ActiveAccount),nameof(ActiveName),nameof(ActiveProvider),nameof(ActiveAvatar),nameof(HasActive),nameof(HasAccounts),nameof(HasMessage),nameof(IsChoice),nameof(IsOffline),nameof(IsThirdParty),nameof(IsProfiles)})Raise(property);
    }
    private void RefreshCommands()
    {
        OfflineCommand.Refresh();ThirdPartyCommand.Refresh();BackCommand.Refresh();
        foreach(var command in new[]{OfflineLoginCommand,AddAuthorityCommand,OnlineLoginCommand,SelectProfileCommand,ActivateCommand,LogoutCommand,RemoveCommand})command?.Refresh();
    }
    public async ValueTask DisposeAsync()
    {
        _lifetime.Cancel();if(_accounts is not null)_accounts.Changed-=OnChanged;Password="";
        await Task.WhenAll(new[]{OfflineLoginCommand,AddAuthorityCommand,OnlineLoginCommand,SelectProfileCommand,ActivateCommand,LogoutCommand,RemoveCommand}.Select(c=>c.DrainAsync()));_lifetime.Dispose();
    }
}
