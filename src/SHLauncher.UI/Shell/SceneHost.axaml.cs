using Avalonia;
using Avalonia.Controls;
using Avalonia.Media;
using AnimationImage;
using SHLauncher.UI.Presentation;

namespace SHLauncher.UI.Shell;

public enum SceneMaterialMode { Solid, Native, InApp }

/// <summary>One player and one current bitmap, shared by the clear and blurred scene draws.</summary>
public sealed partial class SceneHost : UserControl, IDisposable
{
    private BackgroundSession? _session;
    private bool _ownsSession;
    private Image? _blurredDynamic;
    private bool _reduced;
    private bool _active = true;
    private bool _disposed;

    public BackgroundSession Session => _session ?? throw new InvalidOperationException("Scene not initialized.");

    public void Initialize(BackgroundSession session)
    {
        if (_session is not null) throw new InvalidOperationException("Scene already initialized.");
        _session = session;
        StaticFallback.Source = session.Poster;
    }
    public SceneMaterialMode MaterialMode { get; private set; } = SceneMaterialMode.InApp;

    public SceneHost()
    {
        InitializeComponent();

        // The library reuses a WriteableBitmap. Share that frame and invalidate its second draw;
        // do not create another player, decoder, stream, timer or independent playback clock.
        DynamicImage.PropertyChanged += (_, e) =>
        {
            if (_blurredDynamic is null) return;
            if (e.Property == Image.SourceProperty) _blurredDynamic.Source = DynamicImage.Source;
            if (e.Property == AnimatableBitmap.AnimationTimeProperty) _blurredDynamic.InvalidateVisual();
        };
        Loaded += (_, _) => { if (_session is null) { Initialize(new BackgroundSession()); _ownsSession = true; } SetMaterialMode(MaterialMode); ApplyPlayback(); };
    }

    public void SetMaterialMode(SceneMaterialMode mode)
    {
        MaterialMode = mode;
        _blurredDynamic = null;
        MaterialSurface.Child = mode switch
        {
            SceneMaterialMode.Solid => new Border { Background = Brush.Parse("#101115") },
            SceneMaterialMode.Native => new ExperimentalAcrylicBorder
            {
                Material = new ExperimentalAcrylicMaterial
                {
                    TintColor = Color.Parse("#101520"), TintOpacity = 0.84,
                    MaterialOpacity = 0.84, FallbackColor = Color.Parse("#101520")
                }
            },
            _ => CreateSharedFrameMaterial()
        };
    }

    private Control CreateSharedFrameMaterial()
    {
        var blurredPoster = new Image { Source = Session.Poster, Stretch = Stretch.UniformToFill };
        _blurredDynamic = new Image
        {
            Source = DynamicImage.Source, Stretch = Stretch.UniformToFill,
            IsVisible = DynamicImage.IsVisible
        };
        var frames = new Grid { Effect = new BlurEffect { Radius = Resource<double>("SHLN.GlassBlur") } };
        frames.Children.Add(blurredPoster);
        frames.Children.Add(_blurredDynamic);
        var result = new Grid();
        result.Children.Add(frames);
        result.Children.Add(new Border { Background = Brush.Parse("#10274870") });
        result.Children.Add(new Border
        {
            Background = new LinearGradientBrush
            {
                StartPoint = new RelativePoint(0, 0, RelativeUnit.Relative),
                EndPoint = new RelativePoint(0, 1, RelativeUnit.Relative),
                GradientStops =
                {
                    new GradientStop(Color.Parse("#1A080E18"), 0),
                    new GradientStop(Colors.Transparent, 0.45),
                    new GradientStop(Color.Parse("#B8080E18"), 1)
                }
            }
        });
        result.Children.Add(new Border { Background = new SolidColorBrush(Resource<Color>("SHLN.GlassTint")) });
        return result;
    }

    private T Resource<T>(string key) => this.FindResource(key) is T value
        ? value : throw new InvalidOperationException("Missing shell token: " + key);

    public bool SharesCurrentFrame => _blurredDynamic is not null &&
        DynamicImage.Source is not null && ReferenceEquals(_blurredDynamic.Source, DynamicImage.Source);

    public void SetMaterialVisible(bool visible) => MaterialLayer.IsVisible = visible;

    public void Configure(bool reduced, bool active)
    {
        _reduced = reduced;
        _active = active;
        ApplyPlayback();
    }

    private void ApplyPlayback()
    {
        if (_disposed || !IsLoaded) return;
        if (!_reduced && _active) Session.Attach(DynamicImage);
        DynamicImage.IsVisible = !_reduced && Session.Ready;
        if (_blurredDynamic is not null) _blurredDynamic.IsVisible = DynamicImage.IsVisible;
        Session.Configure(_reduced, _active, false);
    }

    protected override Size MeasureOverride(Size availableSize)
    {
        base.MeasureOverride(availableSize);
        return default;
    }

    public void ClearSources()
    {
        _active=false;ApplyPlayback();DynamicImage.Source=null;StaticFallback.Source=null;
        if(_blurredDynamic is not null)_blurredDynamic.Source=null;MaterialSurface.Child=null;
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        if (_ownsSession) _session?.Dispose();
    }
}
