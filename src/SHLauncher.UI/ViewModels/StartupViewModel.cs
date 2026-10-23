using SHLauncher.Application.Accounts;
using SHLauncher.UI.Presentation;
namespace SHLauncher.UI.ViewModels;
public enum RootScreen{StartupSplash,AccountGate,MainShell}
public sealed class StartupViewModel:ObservableModel
{
    private readonly AccountCoordinator? _accounts;
    private RootScreen _screen=RootScreen.StartupSplash;
    public RootScreen Screen{get=>_screen;private set{if(Set(ref _screen,value))ScreenChanged?.Invoke();}}
    public AccountsViewModel Accounts{get;}
    public event Action? ScreenChanged;
    public DelegateCommand ManageAccountsCommand{get;}
    public DelegateCommand ReturnCommand{get;}
    public StartupViewModel(AccountCoordinator? accounts)
    {
        _accounts=accounts;Accounts=new(accounts);Accounts.Authenticated+=()=>Screen=RootScreen.MainShell;
        ManageAccountsCommand=new(()=>{Accounts.CanReturn=true;Screen=RootScreen.AccountGate;});
        ReturnCommand=new(()=>{if(accounts?.Active is not null)Screen=RootScreen.MainShell;});
        Accounts.ReturnCommand=ReturnCommand;
    }
    public async Task InitializeAsync(CancellationToken token)
    {
        try{Screen=_accounts is not null&&await _accounts.InitializeAsync(token)?RootScreen.MainShell:RootScreen.AccountGate;}
        catch(Exception error){Accounts.Report(error);Screen=RootScreen.AccountGate;}
    }
}
