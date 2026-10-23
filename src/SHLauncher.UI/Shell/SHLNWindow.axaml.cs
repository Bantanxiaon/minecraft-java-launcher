using Avalonia;
using Avalonia.Animation;
using Avalonia.Controls;
using Avalonia.Controls.Notifications;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.VisualTree;
using Ursa.Common;
using Ursa.Controls;
using Ursa.Controls.Options;
using Ursa.Controls.OverlayShared;

namespace SHLauncher.UI.Shell;

/// <summary>Presentation host only. Ursa owns overlay lifetime; Avalonia owns window behavior.</summary>
public sealed partial class SHLNWindow : Window
{
    private WindowToastManager? _toasts;
    private bool _closed;
    private bool _workspace;
    public void SetWorkspace(bool workspace){_workspace=workspace;WorkspaceBackdrop.IsVisible=workspace;UpdateScene();}

    public SceneHost Scene => SceneLayer;
    public bool ReducedMotion { get; private set; }

    public SHLNWindow()
    {
        InitializeComponent();
        SetReducedMotion(false);
        AddHandler(KeyDownEvent, OnShellKeyDown, RoutingStrategies.Tunnel);
        Opened += (_, _) =>
        {
            _toasts = new WindowToastManager(this)
            {
                MaxItems = 3,
                HorizontalAlignment = global::Avalonia.Layout.HorizontalAlignment.Right,
                VerticalAlignment = global::Avalonia.Layout.VerticalAlignment.Top,
                Margin = new Thickness(20, 56, 20, 20)
            };
            UpdateScene();
        };
        Closed += (_, _) =>
        {
            _closed = true;
            _toasts?.CloseAll();
            _toasts?.Uninstall();
            SceneLayer.Dispose();
        };
        PropertyChanged += (_, e) =>
        {
            if (e.Property == WindowStateProperty || e.Property == IsVisibleProperty)
                UpdateScene();
        };
    }

    private void OnShellKeyDown(object? sender, KeyEventArgs e)
    {
        if (e.Key != Key.Escape || !OverlayHost.IsInModalStatus)
            return;
        var top = OverlayHost.Children.OfType<OverlayFeedbackElement>().LastOrDefault();
        if (top is null)
            return;
        top.Close();
        e.Handled = true;
    }

    private void UpdateScene()
    {
        WindowSurface.CornerRadius = WindowState == WindowState.Maximized
            ? new CornerRadius(0) : (CornerRadius)this.FindResource("SHLN.WindowRadius")!;
        SceneLayer.Configure(ReducedMotion, IsVisible && !_workspace && WindowState != WindowState.Minimized);
    }

    public void SetReducedMotion(bool reduced)
    {
        ReducedMotion = reduced;
        Classes.Set("reduced", reduced);
        PageHost.PageTransition = new CrossFade(reduced
            ? TimeSpan.Zero : (TimeSpan)this.FindResource("SHLN.MotionStandard")!);
        OverlayHost.IsAnimationDisabled = reduced;
        foreach (var toast in this.GetVisualDescendants().OfType<ToastCard>())
            toast.Classes.Set("reduced", reduced);
        UpdateScene();
    }

    public void SetSceneMaterial(SceneMaterialMode mode)
    {
        SceneLayer.SetMaterialMode(mode);
        TransparencyLevelHint = mode == SceneMaterialMode.Native
            ? [WindowTransparencyLevel.Mica, WindowTransparencyLevel.AcrylicBlur, WindowTransparencyLevel.None]
            : [WindowTransparencyLevel.None];
    }

    public void SetPage(Control content, bool scenic = true)
    {
        PageHost.Content = content;
        SceneLayer.SetMaterialVisible(scenic);
    }

    public void SetNavigation(IEnumerable<Control> controls)
    {
        PrototypeNavigation.Children.Clear();
        NavigationLayer.IsVisible = false;
        foreach (var control in controls)
            PrototypeNavigation.Children.Add(control);
        NavigationLayer.IsVisible = PrototypeNavigation.Children.Count > 0;
    }

    // Ursa supplies modal focus trapping. The adapter restores the invoker after close.
    private async Task<DialogResult> WithFocusReturn(Func<Task<DialogResult>> show)
    {
        var previous = FocusManager?.GetFocusedElement() as Control;
        Task? removed = null;
        OverlayFeedbackElement? surface = null;
        EventHandler<VisualTreeAttachmentEventArgs>? onDetached = null;
        try
        {
            var result = show();
            surface = OverlayHost.Children.OfType<OverlayFeedbackElement>().LastOrDefault();
            if (surface is not null)
            {
                var detached = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
                onDetached = (_, _) => detached.TrySetResult();
                surface.DetachedFromVisualTree += onDetached;
                removed = detached.Task;
            }
            var value = await result;
            // Ursa returns its result before the exit animation removes the visual.
            // Restore focus only once the actual modal surface has left the tree.
            if (removed is not null) await removed;
            return value;
        }
        finally
        {
            if (surface is not null && onDetached is not null)
                surface.DetachedFromVisualTree -= onDetached;
            if (!_closed && previous is { IsEffectivelyVisible: true, IsEffectivelyEnabled: true })
                previous.Focus();
        }
    }

    public Task<DialogResult> ShowDialogAsync(string title, Control content) => WithFocusReturn(() =>
        OverlayDialog.ShowStandardAsync(content, null, "SHLN", new OverlayDialogOptions
        {
            Title = title, Buttons = DialogButton.OKCancel, CanDragMove = false,
            CanLightDismiss = false, TopLevelHashCode = GetHashCode(),
            HorizontalScrollBarVisibility = global::Avalonia.Controls.Primitives.ScrollBarVisibility.Disabled
        }));

    public Task<DialogResult> ShowSheetAsync(string title, Control content, bool customActions = false) => WithFocusReturn(() =>
        OverlayDrawer.ShowStandardAsync(content, null, "SHLN", new DrawerOptions
        {
            Title = title, Position = Ursa.Common.Position.Bottom, Buttons = customActions ? DialogButton.None : DialogButton.OKCancel,
            CanLightDismiss = false, MaxHeight = 480, TopLevelHashCode = GetHashCode()
        }));

    public Task<DialogResult> ShowDrawerAsync(string title, Control content) => WithFocusReturn(() =>
        OverlayDrawer.ShowStandardAsync(content, null, "SHLN", new DrawerOptions
        {
            Title = title, Position = Ursa.Common.Position.Right, Buttons = DialogButton.None,
            CanLightDismiss = true, MinWidth = 360, MaxWidth = 440, TopLevelHashCode = GetHashCode()
        }));

    public void ShowToast(string text) => _toasts?.Show(text, NotificationType.Information,
        TimeSpan.FromSeconds(4), showClose: true, classes: ReducedMotion ? ["reduced"] : null);
}
