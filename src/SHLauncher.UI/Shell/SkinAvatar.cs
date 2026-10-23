using Avalonia;
using Avalonia.Controls;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Avalonia.Platform;
namespace SHLauncher.UI.Shell;
public sealed class SkinAvatar:Control,IDisposable
{
    private Bitmap? _skin;
    private byte[]? _bytes;
    public SkinAvatar(){Width=28;Height=28;RenderOptions.SetBitmapInterpolationMode(this,BitmapInterpolationMode.None);SetSkin(null);}
    public void SetSkin(byte[]? bytes)
    {
        _bytes=bytes;
        Bitmap? bitmap=null;
        try{using var stream=bytes is null?AssetLoader.Open(new Uri("avares://SHLauncher.UI/Assets/Skins/shln-default.png")):new MemoryStream(bytes);bitmap=new Bitmap(stream);if(bitmap.PixelSize.Width!=64||bitmap.PixelSize.Height is not (32 or 64))throw new InvalidDataException();}
        catch{bitmap?.Dispose();using var stream=AssetLoader.Open(new Uri("avares://SHLauncher.UI/Assets/Skins/shln-default.png"));bitmap=new Bitmap(stream);}
        var previous=_skin;_skin=bitmap;InvalidateVisual();previous?.Dispose();
    }
    protected override void OnAttachedToVisualTree(VisualTreeAttachmentEventArgs e){base.OnAttachedToVisualTree(e);if(_skin is null)SetSkin(_bytes);}
    protected override void OnDetachedFromVisualTree(VisualTreeAttachmentEventArgs e){base.OnDetachedFromVisualTree(e);Dispose();}
    public override void Render(DrawingContext context)
    {
        base.Render(context);if(_skin is null)return;var target=new Rect(Bounds.Size);
        context.DrawImage(_skin,new Rect(8,8,8,8),target);context.DrawImage(_skin,new Rect(40,8,8,8),target);
    }
    public void Dispose(){_skin?.Dispose();_skin=null;}
}
