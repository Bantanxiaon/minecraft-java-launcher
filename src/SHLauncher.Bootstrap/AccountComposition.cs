using SHLauncher.Application.Accounts;
using SHLauncher.Contracts.Accounts;
using SHLauncher.Contracts.Engine;
using SHLauncher.Core.Launching;
using SHLauncher.Downloads;
using SHLauncher.Infrastructure.Accounts;
namespace SHLauncher.Bootstrap;
public sealed partial class LauncherComposition
{
    private AuthenticationTransport _accountTransport=null!;
    private IAuthenticationAgentService _authenticationAgent=null!;
    public AccountCoordinator Accounts{get;private set;}=null!;
    private void InitializeAccounts()
    {
        _accountTransport=new();var yggdrasil=new YggdrasilAccountProvider(_accountTransport);
        _authenticationAgent=new AuthlibInjectorService(_downloads,new JsonMetadataSource(_downloads,Path.Combine(_workspace,"shared","metadata")),_workspace);
        Accounts=new(new JsonAccountCatalogStore(_workspace),new ProtectedCredentialStore(_workspace,new SHLauncher.Platform.WindowsCredentialProtector()),
            [new OfflineMinecraftAccountProvider(),yggdrasil,new MicrosoftAccountProvider()],yggdrasil,
            ()=>Coordinator.Snapshot.State is LauncherTaskState.Idle or LauncherTaskState.Failed);
    }
}
