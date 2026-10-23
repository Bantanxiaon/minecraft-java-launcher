using SHLauncher.Contracts.Install;
using SHLauncher.UI.Presentation;
namespace SHLauncher.UI.ViewModels;
public sealed partial class DownloadsViewModel
{
    private string _resolutionMessage="",_resolutionDetails="";
    private bool _catalogFailed;
    public string ResolutionMessage=>_resolutionMessage;
    public string ResolutionDetails=>_resolutionDetails;
    public bool HasResolutionProblem=>_resolutionMessage.Length!=0;
    public AsyncCommand ResolutionRetryCommand {get;}
    public string InstallBlocker
    {
        get {
            if(_disposed||_workspace is null)return "安装服务尚未就绪。";
            if(Busy)return "正在刷新版本信息，请稍候。";
            if(Minecraft is null)return "先刷新并选择 Minecraft 版本。";
            if(Minecraft.Kind!=MinecraftVersionKind.Release)return "当前仅支持安装正式版，请切换版本类型。";
            if(NeedsLoader&&LoaderVersion is null)
                return _loadingLoaders?"正在获取兼容的加载器版本。":HasResolutionProblem?"兼容信息尚未确认，请先重试。":"未找到兼容的加载器版本，请更换游戏版本或加载器。";
            if(string.IsNullOrWhiteSpace(DirectoryName))return "为新实例填写一个名称，即可安装。";
            if(DirectoryName.Length>64)return "实例名称最多为 64 个字符。";
            return "";
        }
    }
    private void RefreshInstallState()
    {
        InstallCommand.Refresh();ResolutionRetryCommand.Refresh();Raise(nameof(InstallBlocker));
    }
    private void ClearResolutionProblem()
    {
        _resolutionMessage="";_resolutionDetails="";_catalogFailed=false;RefreshResolution();
    }
    private void SetResolutionProblem(string message,Exception error,string context)
    {
        _resolutionMessage=message;_catalogFailed=context=="Minecraft 官方目录";
        _resolutionDetails=context+Environment.NewLine+error.GetType().Name+": "+error.Message;
        RefreshResolution();
    }
    private void RefreshResolution()
    {
        Raise(nameof(ResolutionMessage));Raise(nameof(ResolutionDetails));Raise(nameof(HasResolutionProblem));RefreshInstallState();
    }
}
