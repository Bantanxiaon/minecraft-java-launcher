using Avalonia.Controls;
using Avalonia.Interactivity;
using Avalonia.VisualTree;
using SHLauncher.UI.Shell;
using SHLauncher.UI.ViewModels;
using Ursa.Controls.OverlayShared;
namespace SHLauncher.UI.Views.Pages;
public sealed partial class DownloadsView:UserControl
{
    private bool _installOpen;
    public DownloadsView(){InitializeComponent();Loaded+=async(_,_)=>{if(DataContext is DownloadsViewModel vm&&vm.MinecraftVersions.Count==0)await vm.RefreshAsync();};}
    private async void OpenInstall(object? sender,RoutedEventArgs e)
    {
        if(_installOpen||DataContext is not DownloadsViewModel vm||TopLevel.GetTopLevel(this) is not SHLNWindow window)return;
        if(vm.Minecraft is null){window.ShowToast("请选择 Minecraft 版本");return;}
        _installOpen=true;var options=new InstallOptions{DataContext=vm};
        void Queued(){options.GetVisualAncestors().OfType<OverlayFeedbackElement>().FirstOrDefault()?.Close();window.ShowToast("已加入下载任务");}
        vm.InstallQueued+=Queued;
        try{await window.ShowSheetAsync("安装 Minecraft",options,true);}
        finally{vm.InstallQueued-=Queued;_installOpen=false;}
    }
}
