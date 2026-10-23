using Avalonia.Controls;
using Avalonia.Interactivity;
using Avalonia.Platform.Storage;
using SHLauncher.UI.ViewModels;
using Ursa.Controls;
namespace SHLauncher.UI.Shell;
public sealed partial class ModsSurface:UserControl
{
    public ModsSurface(){InitializeComponent();Loaded+=(_,_)=>Model?.Activate();Unloaded+=(_,_)=>Model?.Deactivate();}
    private ModsViewModel? Model=>DataContext as ModsViewModel;
    private async void Online(object? sender,RoutedEventArgs e)
    {
        if(TopLevel.GetTopLevel(this) is not SHLNWindow window||Model?.Online is not {} online)return;
        try{Model.PrepareOnline();await window.ShowSheetAsync("在线 Mod",new OnlineModsSurface{DataContext=online},true);}catch(Exception error){window.ShowToast(error.Message);}
    }
    private async void AddLocal(object? sender,RoutedEventArgs e)
    {
        if(TopLevel.GetTopLevel(this) is not SHLNWindow window||Model is not {} model)return;
        var files=await window.StorageProvider.OpenFilePickerAsync(new(){Title="添加本地 Mod",AllowMultiple=true,FileTypeFilter=[new("Minecraft Mod"){Patterns=["*.jar"]}]});
        await model.InstallAsync(files.Select(f=>f.TryGetLocalPath()).OfType<string>().ToArray());
    }
    private async void Recycle(object? sender,RoutedEventArgs e)
    {
        if(TopLevel.GetTopLevel(this) is not SHLNWindow window||Model is not {} model)return;
        if(await window.ShowDialogAsync("移入回收站？",new TextBlock{Text=$"将处理选中的 {model.SelectedCount} 个 Mod，失败的文件会保留。"})==DialogResult.OK)
            try{await model.RecycleSelectedAsync();}catch(Exception error){window.ShowToast(error.Message);}
    }
    private async void Details(object? sender,RoutedEventArgs e)
    {
        if(sender is not Button{DataContext:ModRow row}||TopLevel.GetTopLevel(this) is not SHLNWindow window)return;
        var mod=row.Mod;await window.ShowSheetAsync(mod.Name,new ScrollViewer{Content=new TextBlock{Text=$"{mod.Id} · {mod.Version} · {mod.Loader}\n作者：{string.Join(", ",mod.Authors)}\n{mod.Description}\n\n依赖\n"+string.Join("\n",mod.Dependencies.Select(d=>$"{d.Id} {d.Constraint}"))+"\n\n"+mod.Path,TextWrapping=Avalonia.Media.TextWrapping.Wrap,MaxWidth=680}},true);
    }
}
