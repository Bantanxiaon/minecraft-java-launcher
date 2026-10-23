using Avalonia;
using Avalonia.Controls;
using SHLauncher.UI.ViewModels;
namespace SHLauncher.UI.Shell;
public sealed class SavedAccountAvatar:UserControl
{
    private readonly SkinAvatar _avatar=new(){Width=32,Height=32};
    private CancellationTokenSource? _loading;private bool _attached;
    public SavedAccountAvatar(){Content=_avatar;DataContextChanged+=(_,_)=>Reload();}
    protected override void OnAttachedToVisualTree(VisualTreeAttachmentEventArgs e){base.OnAttachedToVisualTree(e);_attached=true;Reload();}
    protected override void OnDetachedFromVisualTree(VisualTreeAttachmentEventArgs e){_attached=false;_loading?.Cancel();_avatar.Dispose();base.OnDetachedFromVisualTree(e);}
    private async void Reload()
    {
        _loading?.Cancel();if(!_attached)return;
        using var loading=new CancellationTokenSource();_loading=loading;_avatar.SetSkin(null);
        try{if(DataContext is SavedAccountRow account){var skin=await account.LoadSkinAsync(loading.Token);if(_attached&&!loading.IsCancellationRequested)_avatar.SetSkin(skin);}}
        catch(OperationCanceledException){}catch{ /* A texture failure never blocks account selection. */ }
        finally{if(ReferenceEquals(_loading,loading))_loading=null;}
    }
}
