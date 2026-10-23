using Avalonia;
using Avalonia.Media.Imaging;
using Avalonia.Platform;
using AnimationImage;
namespace SHLauncher.UI.Presentation;

// One session choice and one library player; never owns a launcher/account task.
public sealed class BackgroundSession : IDisposable
{
    public static IReadOnlyList<string> Approved { get; } = ["polar-trails"];
    public string Id { get; }
    public Bitmap Poster { get; }
    private AnimatableBitmap? _player;
    private Avalonia.Controls.Image? _target;
    // AnimationImage exposes milliseconds; the UI audit samples seconds.
    public double CurrentSeconds => (_player?.CurrentTime ?? 0) / 1000d;
    public bool IsPlaying => _player?.State == AnimationState.Playing;
    public bool Ready => _player is not null && _player.State != AnimationState.Error;
    public string? Failure { get; private set; }
    private bool _disposed;
    public BackgroundSession(string? previous = null, Action<string>? save = null)
    {
        var choices=Approved.Where(x=>x!=previous).ToArray();
        Id=choices.Length>0?choices[Random.Shared.Next(choices.Length)]:Approved[0];
        try{using var stream=AssetLoader.Open(Asset(Id+".jpg"));Poster=new Bitmap(stream);}
        catch{using var stream=AssetLoader.Open(Asset("twilight-lake.png"));Poster=new Bitmap(stream);Failure="背景素材不可用";}
        save?.Invoke(Id);
    }
    public static Uri Asset(string name)=>new("avares://SHLauncher.UI/Assets/Backgrounds/"+name);
    public void Attach(Avalonia.Controls.Image target)
    {
        _target=target;
        if(_disposed||_player is not null||Failure is not null)return;
        try
        {
            AnimatableBitmap.SetAutoStart(target,false);
            AnimatableBitmap.SetRepeatBehavior(target,RepeatBehavior.Forever);
            var player=AnimatableBitmapFactory.Default.Create(new AnimatableBitmapOptions(Asset(Id+".webp"),useGPU:false,preload:false));
            if(player.State==AnimationState.Error){player.Dispose();Failure="背景播放不可用";return;}
            _player=player;
            AnimatableBitmap.SetSource(target,player);
        }
        catch{_player?.Dispose();_player=null;Failure="背景播放不可用";}
    }
    public void Configure(bool reduced,bool visible,bool minimized)
    {
        if(!_disposed&&_player is null&&_target is not null&&!reduced&&visible&&!minimized)Attach(_target);
        if(_disposed||_player is null)return;
        if(!reduced&&visible&&!minimized)
        {
            if(_player.BeginCommand.CanExecute(null))_player.BeginCommand.Execute(null);
        }
        else if(_player.PauseCommand.CanExecute(null))_player.PauseCommand.Execute(null);
    }
    public bool ReleasePausedPlayer(){if(_disposed||_player is null||IsPlaying)return false;if(_target is not null){_target.Source=null;AnimatableBitmap.SetSource(_target,null);}_player.Dispose();_player=null;return true;}
    public void Dispose(){if(_disposed)return;_disposed=true;_player?.Dispose();_player=null;Poster.Dispose();}
}
