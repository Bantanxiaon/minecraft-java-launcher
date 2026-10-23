using System.Diagnostics;
using System.Text.Json;
using Avalonia;
using Avalonia.Animation;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Input;
using Avalonia.Media.Imaging;
using Avalonia.VisualTree;
using SHLauncher.UI.Shell;
using Ursa.Controls;
using Ursa.Controls.OverlayShared;

internal static class ProbeAudit
{
    internal static async Task Run(SHLNWindow window, string output)
    {
        Directory.CreateDirectory(output);
        var checks = new List<string>();
        var samples = new List<object>();
        var process = Process.GetCurrentProcess();
        void Check(bool value, string label)
        {
            if (!value) throw new InvalidOperationException(label);
            checks.Add(label);
        }
        void Frame(string label, double scale = 1)
        {
            window.UpdateLayout();
            using var bitmap = new RenderTargetBitmap(
                new PixelSize((int)(window.Bounds.Width * scale), (int)(window.Bounds.Height * scale)),
                new Vector(96 * scale, 96 * scale));
            bitmap.Render(window);
            bitmap.Save(Path.Combine(output, label + ".png"), PngBitmapEncoderOptions.Default);
        }
        async Task Shot(string label, double scale = 1)
        {
            await Task.Delay(300);
            Frame(label, scale);
        }
        async Task Measure(string label, int seconds)
        {
            var clock = Stopwatch.StartNew();
            var cpu = process.TotalProcessorTime;
            for (var i = 0; i < seconds; i++)
            {
                await Task.Delay(1000);
                process.Refresh();
                samples.Add(new
                {
                    Mode = window.Scene.MaterialMode.ToString(), Stage = label,
                    Seconds = clock.Elapsed.TotalSeconds,
                    CpuMs = (process.TotalProcessorTime - cpu).TotalMilliseconds,
                    WorkingSet = process.WorkingSet64, Private = process.PrivateMemorySize64,
                    MediaSeconds = window.Scene.Session.CurrentSeconds, window.RenderScaling,
                    TransparencyLevel = window.ActualTransparencyLevel.ToString()
                });
            }
        }
        OverlayFeedbackElement CurrentOverlay() =>
            window.GetVisualDescendants().OfType<OverlayFeedbackElement>().Last();
        async Task CloseWithEscape(Task<Ursa.Controls.DialogResult> task, Control invoker)
        {
            var overlay = CurrentOverlay();
            Check(KeyboardNavigation.GetTabNavigation(overlay) == KeyboardNavigationMode.Cycle,
                "modal uses keyboard focus cycle");
            var ready = Stopwatch.StartNew();
            bool HasOverlayFocus() => window.FocusManager?.GetFocusedElement() is Control focus &&
                focus.GetVisualAncestors().Contains(overlay);
            while (!HasOverlayFocus() && ready.Elapsed < TimeSpan.FromSeconds(2))
                await Task.Delay(25);
            Check(HasOverlayFocus(), "modal receives initial keyboard focus");
            window.RaiseEvent(new KeyEventArgs { RoutedEvent = InputElement.KeyDownEvent, Key = Key.Escape });
            await task.WaitAsync(TimeSpan.FromSeconds(5));
            await Task.Delay(300);
            Check(ReferenceEquals(window.FocusManager?.GetFocusedElement(), invoker),
                "Escape closes overlay and returns focus to invoker");
        }
        try
        {
            await Task.Delay(1800);
            Check(window.Scene.Session.Ready, "shared star-trail player decoded");
            Check(window.Scene.Session.IsPlaying, "shared star-trail player is playing");
            if(window.Scene.MaterialMode==SceneMaterialMode.InApp) Check(window.Scene.SharesCurrentFrame,"clear and blurred views share one decoded frame");
            await Shot("restored");
            await Measure("playing", 10);
            await Shot("motion-later");
            window.SetReducedMotion(true);
            var time = window.Scene.Session.CurrentSeconds;
            await Measure("reduced-idle", 4);
            Check(Math.Abs(window.Scene.Session.CurrentSeconds - time) < 0.01,
                "Reduced Motion freezes scene");
            Check(((CrossFade)window.FindControl<TransitioningContentControl>("PageHost")!.PageTransition!).Duration == TimeSpan.Zero,
                "Reduced Motion removes page transition");
            await Shot("static-fallback");
            window.ShowToast("减少动态 · 通知示例");
            await Task.Delay(80);
            var toast = window.GetVisualDescendants().OfType<ToastCard>().Single();
            Check(toast.Classes.Contains("reduced"), "Reduced Motion reaches Ursa toast theme");
            await Shot("reduced-toast");
            toast.Close();
            await Task.Delay(80);
            Check(!window.GetVisualDescendants().OfType<ToastCard>().Contains(toast),
                "reduced toast closes without waiting for an animation");
            window.SetReducedMotion(false);

            window.Width = 900; window.Height = 640; await Shot("resized-small");
            window.Width = 1280; window.Height = 800; await Shot("resized-large");
            window.WindowState = WindowState.Maximized; await Shot("maximized");
            Check(window.WindowState == WindowState.Maximized, "maximize");
            window.WindowState = WindowState.Normal; await Shot("restored-again");
            Check(window.WindowState == WindowState.Normal, "restore");
            window.WindowState = WindowState.Minimized; await Task.Delay(200);
            Check(!window.Scene.Session.IsPlaying, "minimize pauses scene");
            window.WindowState = WindowState.Normal; await Task.Delay(250);
            Check(window.Scene.Session.IsPlaying, "restore resumes scene");
            var invoker = window.GetVisualDescendants().OfType<Button>().First(x => Equals(x.Content, "弹窗"));
            invoker.Focus();
            var dialog = window.ShowDialogAsync("操作确认",
                new TextBlock { Text = "统一 Dialog · 不修改任何业务数据", Margin = new Thickness(24) });
            await Shot("dialog");
            var closeButtons = CurrentOverlay().GetVisualDescendants().OfType<Button>().Where(x => x.Name == "PART_CloseButton").ToArray();
            Check(closeButtons.Length > 0 && closeButtons.All(x => AutomationProperties.GetName(x) == "关闭"), "overlay close controls have a readable accessibility name");
            Check(((IClassicDesktopStyleApplicationLifetime)Application.Current!.ApplicationLifetime!).Windows.Count == 1,
                "overlays create no separate OS window");
            await CloseWithEscape(dialog, invoker);
            invoker.Focus();
            var sheet = window.ShowSheetAsync("表单示例", new StackPanel
            {
                Margin = new Thickness(24), Spacing = 12,
                Children = { new TextBlock { Text = "统一 Sheet", FontSize = 24 },
                    new TextBox { PlaceholderText = "输入示例", MinWidth = 360 } }
            });
            await Shot("sheet");
            window.Width = 900; window.Height = 640; await Shot("sheet-resize");
            Check(CurrentOverlay().Bounds.Width <= window.Bounds.Width,
                "sheet stays within resized window");
            await CloseWithEscape(sheet, invoker);
            window.Width = 1280; window.Height = 800;
            invoker.Focus();
            var drawer = window.ShowDrawerAsync("任务中心",
                new TextBlock { Text = "暂无任务 · 外壳原型", Margin = new Thickness(24) });
            await Shot("drawer");
            await CloseWithEscape(drawer, invoker);
            invoker.Focus();
            var quick = window.ShowDrawerAsync("快速关闭", new TextBlock { Text = "快速关闭验收" });
            await Task.Delay(25);
            window.RaiseEvent(new KeyEventArgs { RoutedEvent = InputElement.KeyDownEvent, Key = Key.Escape });
            await quick.WaitAsync(TimeSpan.FromSeconds(5));
            await Task.Delay(500);
            Check(ReferenceEquals(window.FocusManager?.GetFocusedElement(), invoker),
                "Escape during drawer entry preserves invoker focus after detach");
            window.ShowToast("通知示例"); await Shot("toast");

            window.SetPage(new TextBlock
            {
                Text = "页面切换", FontSize = 32, Margin = new Thickness(64),
                VerticalAlignment = global::Avalonia.Layout.VerticalAlignment.Bottom
            });
            Directory.CreateDirectory(Path.Combine(output, "transition"));
            for (var i = 0; i < 10; i++)
            {
                Frame("transition/frame-" + i.ToString("000"));
                await Task.Delay(40);
            }
            await Shot("page-transition");
            foreach (var scale in new[] { 1d, 1.5, 2 })
                await Shot($"render-dpi-{scale * 100:0}", scale);
            var wraps = 0;
            var timeline = new List<object>();
            if (output.Contains("review-2", StringComparison.Ordinal))
            {
                Directory.CreateDirectory(Path.Combine(output, "loop"));
                var previous = window.Scene.Session.CurrentSeconds;
                for (var i = 0; i < 360; i++)
                {
                    var current = window.Scene.Session.CurrentSeconds;
                    if (current < previous - 1) wraps++;
                    timeline.Add(new { Frame = i, MediaSeconds = current });
                    Frame("loop/frame-" + i.ToString("000"));
                    previous = current;
                    await Task.Delay(200);
                }
                Check(wraps >= 2, "shared scene and glass render through two loop wraps");
            }
            await File.WriteAllTextAsync(Path.Combine(output, "result.json"), JsonSerializer.Serialize(new
            {
                Passed = true, Mode = window.Scene.MaterialMode.ToString(), Checks = checks, Samples = samples,
                SharesCurrentFrame = window.Scene.SharesCurrentFrame, Wraps = wraps, Timeline = timeline,
                NativeDpi = "NOT VERIFIED at 150/200%; dpi images are explicit offscreen renders",
                Snap = "Requires native desktop check",
                CaptureScope = "Window client rendering; native caption/hover requires desktop capture"
            }));
        }
        catch (Exception ex)
        {
            await File.WriteAllTextAsync(Path.Combine(output, "result.json"),
                JsonSerializer.Serialize(new { Passed = false, Checks = checks, Failure = ex.ToString(), Samples = samples }));
        }
        window.Close();
    }
}
