using Avalonia.Controls;
using Avalonia.Input;
using SHLauncher.UI.ViewModels;
namespace SHLauncher.UI.Shell;
public sealed partial class AccountSurface : UserControl
{
    public void SetWorkspace(bool workspace) { AccountLayout.ColumnDefinitions=new ColumnDefinitions(workspace?"0,*":"53*,47*");AccountBody.MaxWidth=workspace?640:300; }
    public AccountSurface() { InitializeComponent(); KeyDown += OnKey; }
    private void OnKey(object? sender, KeyEventArgs e)
    {
        if (DataContext is not AccountsViewModel vm || vm.Busy) return;
        if (e.Key == Key.Escape) { if (!vm.IsChoice) vm.BackCommand.Execute(null); else if(vm.CanReturn) vm.ReturnCommand?.Execute(null); e.Handled=true; }
        else if(e.Key == Key.Enter && e.Source is TextBox input) { var command=vm.IsOffline ? vm.OfflineLoginCommand : input.Name=="AuthorityInput" ? vm.AddAuthorityCommand : vm.OnlineLoginCommand; command.Execute(null); e.Handled=true; }
    }
    public void FocusForm() { if(DataContext is AccountsViewModel vm) { if(vm.IsOffline) OfflineNameInput.Focus(); else if(vm.IsThirdParty) IdentifierInput.Focus(); } }
}
