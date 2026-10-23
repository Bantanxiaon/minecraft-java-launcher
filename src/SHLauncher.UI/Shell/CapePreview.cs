using Avalonia;
using Avalonia.Controls;
using Avalonia.Media;
using Avalonia.Media.Imaging;
namespace SHLauncher.UI.Shell;
public sealed class CapePreview:Control,IDisposable
{
    private Bitmap? _texture;
    public CapePreview(){Width=60;Height=96;RenderOptions.SetBitmapInterpolationMode(this,BitmapInterpolationMode.None);}
    public void SetTexture(byte[]? bytes)
    {
        Bitmap? next=null;
        if(bytes is not null){using var stream=new MemoryStream(bytes);next=new Bitmap(stream);}
        var old=_texture;_texture=next;InvalidateVisual();old?.Dispose();
    }
    public override void Render(DrawingContext context)
    {
        base.Render(context);if(_texture is null)return;
        var size=_texture.PixelSize;
        // Standard cape atlas uses a 10 x 16 front at (1,1), scaled with its 64 x 32 canvas.
        var standard=size.Width%64==0&&size.Height==size.Width/2;
        var scale=size.Width/64d;
        var source=standard?new Rect(scale,scale,10*scale,16*scale):new Rect(0,0,size.Width,size.Height);
        context.DrawImage(_texture,source,new Rect(Bounds.Size));
    }
    public void Dispose(){_texture?.Dispose();_texture=null;}
}
