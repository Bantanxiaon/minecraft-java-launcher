using SHLauncher.Contracts.Accounts;
using SHLauncher.UI.Presentation;
namespace SHLauncher.UI.ViewModels;
public sealed class SavedAccountRow(MinecraftAccount account,Action<MinecraftAccount> select,Func<CancellationToken,Task<byte[]?>>? loadSkin=null)
{
    public Task<byte[]?> LoadSkinAsync(CancellationToken token)=>loadSkin?.Invoke(token)??Task.FromResult<byte[]?>(null);
    public string Name=>account.ProfileName;
    public string Avatar=>Name[..1].ToUpperInvariant();
    public string Provider=>account.AccountType==MinecraftAccountType.Offline?"离线账户":account.ProviderDisplayName;
    public DelegateCommand UseCommand{get;}=new(()=>select(account));
}
