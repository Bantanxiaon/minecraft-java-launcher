using SHLauncher.Contracts.Accounts;
using SHLauncher.Infrastructure.Accounts;
namespace SHLauncher.Bootstrap;
public sealed partial class LauncherComposition
{
    private IProfileTextureService? _textures;
    public IProfileTextureService ProfileTextures=>_textures??=new ProfileTextureService(_accountTransport,_workspace);
}
