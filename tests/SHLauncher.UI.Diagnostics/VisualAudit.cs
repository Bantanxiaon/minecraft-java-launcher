using System.Diagnostics;
using System.Text.Json;
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Avalonia.Threading;
using Avalonia.VisualTree;
using SHLauncher.Core.Launching;
using SHLauncher.UI.Components;
using SHLauncher.UI.Presentation;
using SHLauncher.UI.ViewModels;
using SHLauncher.UI.Views;

namespace SHLauncher.UI.Diagnostics;

internal static class VisualAudit
{
    internal static async Task<bool> RunIfRequested(MainWindow window)
    {
        var folder = Environment.GetEnvironmentVariable("SH_UI_VISUAL_DIR");
        if (string.IsNullOrWhiteSpace(folder)) return false;
        Directory.CreateDirectory(folder);
        var checks = new List<string>(); var frames = new List<object>();
        var navigationTrace = new List<object>(); var traceNavigation = false;
        var shell = window.Shell;
        System.ComponentModel.PropertyChangedEventHandler trace = (_, args) =>
        { if (traceNavigation && args.PropertyName == nameof(ShellViewModel.Page)) navigationTrace.Add(new { Page = shell.Page.ToString(), Stack = Environment.StackTrace }); };
        shell.PropertyChanged += trace;
        var watch = Stopwatch.StartNew(); var process = Process.GetCurrentProcess();
        void Check(bool condition, string label)
        { if (!condition) throw new InvalidOperationException(label); checks.Add(label); }
        Button Button(string name) => window.GetVisualDescendants().OfType<Button>().First(x => x.IsEffectivelyVisible && AutomationProperties.GetName(x) == name);
        async Task Click(string name)
        {
            var button = Button(name); Check(button.IsEffectivelyEnabled && button.Command?.CanExecute(null) == true, name + " enabled");
            button.Command!.Execute(null); await Task.Delay(360);
        }
        async Task Wait(LauncherTaskState state)
        {
            using var stop = new CancellationTokenSource(TimeSpan.FromSeconds(14));
            while (shell.Launch.Snapshot.State != state) await Task.Delay(10, stop.Token);
            await Task.Delay(80);
        }
        async Task Resize(Size size)
        {
            window.WindowState = WindowState.Normal; window.Width = size.Width; window.Height = size.Height;
            using var stop = new CancellationTokenSource(TimeSpan.FromSeconds(5));
            while (Math.Abs(window.ClientSize.Width-size.Width)>1 || Math.Abs(window.ClientSize.Height-size.Height)>1)
                await Task.Delay(25, stop.Token);
        }
        async Task Shot(string name, double scale = 1)
        {
            await Task.Delay(380);
            foreach (var scroll in window.GetVisualDescendants().OfType<ScrollViewer>().Where(x => x.IsEffectivelyVisible && AutomationProperties.GetName(x)?.EndsWith("内容", StringComparison.Ordinal) == true))
                Check(scroll.Extent.Width <= scroll.Viewport.Width + 1, name + " no horizontal overflow");
            var pixels = new PixelSize((int)Math.Round(window.Bounds.Width * scale), (int)Math.Round(window.Bounds.Height * scale));
            using var bitmap = new RenderTargetBitmap(pixels, new Vector(96 * scale, 96 * scale));
            bitmap.Render(window); bitmap.Save(Path.Combine(folder, name + ".png"), PngBitmapEncoderOptions.Default);
            frames.Add(new { Name = name, Width = window.ClientSize.Width, Height = window.ClientSize.Height, Scale = scale, ActualRenderScaling = window.RenderScaling, Theme = window.ActualThemeVariant.ToString(), State = shell.Launch.Snapshot.State.ToString() });
        }
        async Task Input(string name, object data)
        {
            File.WriteAllText(Path.Combine(folder, name + "-request.json"), JsonSerializer.Serialize(data));
            using var stop = new CancellationTokenSource(TimeSpan.FromSeconds(20));
            while (!File.Exists(Path.Combine(folder, name + "-ack"))) await Task.Delay(30, stop.Token);
            await Task.Delay(350);
        }
        try
        {
            await Task.Delay(500); shell.Settings.ThemeIndex = 1;
            await Shot("home-light"); shell.Settings.ThemeIndex = 2; await Shot("home-dark");
            for (var theme = 1; theme <= 2; theme++)
            {
                shell.Settings.ThemeIndex = theme; var suffix = theme == 1 ? "light" : "dark";
                await Click("资料库"); await Shot("library-" + suffix);
                await Click("设置"); await Shot("settings-" + suffix);
                await Click("下载");await Shot("downloads-"+suffix);await Click("设置");
                var selector = window.GetVisualDescendants().OfType<ComboBox>().First(x => AutomationProperties.GetName(x) == "主题");
                selector.SelectedIndex = 3 - theme; await Task.Delay(80); Check(shell.Settings.ThemeIndex == 3 - theme, "Theme two-way");
                var motion = window.GetVisualDescendants().OfType<ToggleSwitch>().Single(); motion.IsChecked = true; await Task.Delay(80);
                Check(shell.Settings.ReducedMotion, "Reduced motion binding");
                Avalonia.Application.Current!.Resources.TryGetResource("MotionSlow", null, out var time);
                Check((TimeSpan)time! == TimeSpan.Zero, "Hero animation disabled");
                var advanced=window.GetVisualDescendants().OfType<Expander>().Single(e=>e.Header?.ToString()=="高级");advanced.IsExpanded=true;await Task.Delay(100);
                var slider = window.GetVisualDescendants().OfType<Slider>().Single(); slider.Value = 10240; Check(shell.Settings.MemoryMb == 10240, "Memory preview binding");
                var runtime = window.GetVisualDescendants().OfType<ComboBox>().First(x => AutomationProperties.GetName(x) == "演示运行环境");
                runtime.SelectedIndex = 1; Check(shell.Settings.SelectedRuntime == shell.Settings.Runtimes[1], "Runtime preview binding");
                advanced.IsExpanded=false;motion.IsChecked = false; await Task.Delay(80);
            }
            shell.Settings.ThemeIndex = 0; await Task.Delay(120); Check(shell.Settings.Theme == AppearanceTheme.System, "System theme retained");
            shell.Settings.ThemeIndex = 1; await Click("首页");
            foreach (var name in new[] { "Vanilla Survival", "Create World", "元素觉醒" })
            {
                await Click(name); Check(shell.Home.Hero.InstanceName == name, "Hero theme follows " + name); await Shot("instance-" + shell.Launch.Selected!.Id);
            }
            var themes = shell.Library.Cards.Select(x => x.VisualTheme).Distinct().Count(); Check(themes == 3, "Three presentation-only themes");
            foreach (var size in new[] { new Size(1280,720), new Size(1440,900), new Size(1920,1080), new Size(2560,1440), new Size(900,640) })
            {
                await Resize(size);
                Check(Math.Abs(window.ClientSize.Width - size.Width) < 2 && Math.Abs(window.ClientSize.Height - size.Height) < 2, "Actual client size " + size);
                foreach (var page in new[] { ShellPage.Home, ShellPage.Library, ShellPage.Settings,ShellPage.Downloads })
                {
                    shell.Page = page; await Shot($"size-{(int)size.Width}x{(int)size.Height}-{page.ToString().ToLowerInvariant()}");
                }
            }
            shell.Page = ShellPage.Home; await Shot("narrow-window");
            foreach (var instance in new[] { "vanilla", "create", "elements" })
            { shell.Launch.Select(instance); await Shot("narrow-" + instance); }
            await Shot("dpi125-render", 1.25); await Shot("dpi150-render", 1.5);
            await Resize(new Size(1280, 800));
            Button("最大化或还原").RaiseEvent(new RoutedEventArgs(Avalonia.Controls.Button.ClickEvent)); await Shot("maximized");
            Check(window.WindowState == WindowState.Maximized, "Maximize control");
            Button("最大化或还原").RaiseEvent(new RoutedEventArgs(Avalonia.Controls.Button.ClickEvent)); await Task.Delay(350); Check(window.WindowState == WindowState.Normal, "Restore control");
            Button("最小化").RaiseEvent(new RoutedEventArgs(Avalonia.Controls.Button.ClickEvent)); await Task.Delay(300); Check(window.WindowState == WindowState.Minimized, "Minimize control"); window.WindowState = WindowState.Normal;
            await Task.Delay(300); window.Activate(); await Task.Delay(350);
            if (Environment.GetEnvironmentVariable("SH_UI_NATIVE_INPUT") == "1")
            {
            Check(Button("首页").Focus(NavigationMethod.Tab), "Home receives keyboard focus");
            traceNavigation = true; var libraryButton = Button("资料库"); var tabReachedLibrary = false;
            EventHandler<FocusChangedEventArgs> recordFocus = (_, args) => tabReachedLibrary |= args.NavigationMethod == NavigationMethod.Tab;
            libraryButton.GotFocus += recordFocus;
            try { await Input("keyboard", new { Pid = Environment.ProcessId }); }
            finally { libraryButton.GotFocus -= recordFocus; }
            Check(tabReachedLibrary, "Tab reaches Library navigation");
            Check(shell.Page == ShellPage.Library, "Window-targeted Space activates Library navigation");
            libraryButton.Focus(NavigationMethod.Tab); await Shot("keyboard-focus");
            var hover = Button("元素觉醒"); var point = hover.PointToScreen(new Point(hover.Bounds.Width / 2, 70));
            await Input("hover", new { X = point.X, Y = point.Y }); Check(hover.IsPointerOver, "OS pointer hover with verified window hit");
            shell.Settings.ReducedMotion = true; await Task.Delay(150);
            var transform = hover.RenderTransform!.Value; Check(Math.Abs(transform.M11 - 1) < .001, "Reduced motion removes hover scale");
            await Input("pointer-away", new { X = 10, Y = 10 }); shell.Settings.ReducedMotion = false;
            }
            else
            {
                await Click("资料库");
                Check(Button("资料库").Focus(NavigationMethod.Tab), "Navigation accepts keyboard focus");
                await Shot("keyboard-focus");
                var hover = Button("元素觉醒"); var pseudo = (IPseudoClasses)hover.Classes;
                try
                {
                    for (var i=0;i<30;i++) { pseudo.Add(":pointerover"); await Task.Delay(15); pseudo.Remove(":pointerover"); await Task.Delay(15); }
                    pseudo.Add(":pointerover"); await Task.Delay(180);
                    Check(hover.RenderTransform!.Value.M11 > 1, "Hover style scales gently");
                    shell.Settings.ReducedMotion = true; await Task.Delay(180);
                    Check(Math.Abs(hover.RenderTransform!.Value.M11-1)<.001, "Reduced motion removes hover scale");
                    await Shot("reduced-motion");
                }
                finally { pseudo.Remove(":pointerover"); shell.Settings.ReducedMotion = false; }
            }
            await Click("首页");
            var failure = Environment.GetEnvironmentVariable("SH_MOCK_SCENARIO") == "LaunchFailure";
            Button("开始或结束游戏").Command!.Execute(null);
            if (failure)
            {
                await Wait(LauncherTaskState.Failed); await Shot("failed"); Check(shell.Launch.ShowError, "Inline Hero failure");
                shell.Settings.ThemeIndex = 2; await Shot("failed-dark"); shell.Settings.ThemeIndex = 1;
                await Click("开始或结束游戏"); await Wait(LauncherTaskState.Running); await Shot("retry-running");
                shell.Launch.Cancel(); await Wait(LauncherTaskState.Idle); Check(!shell.Launch.ShowProgress, "Retry and stop complete");
            }
            else
            {
                for (var i=0;i<30;i++) Button("开始或结束游戏").Command!.Execute(null);
                await Wait(LauncherTaskState.Launching); await Shot("launching"); shell.Launch.Cancel(); await Wait(LauncherTaskState.Idle); await Shot("cancelled");
                await Click("开始或结束游戏"); await Wait(LauncherTaskState.Running); await Shot("running");
                await Wait(LauncherTaskState.Idle); Check(!shell.Launch.ShowProgress, "Natural exit no stale progress");
            }
            traceNavigation = false; var gaps = new List<double>(); var last = watch.Elapsed.TotalMilliseconds;
            var heartbeat = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(16) };
            heartbeat.Tick += (_, _) => { var now = watch.Elapsed.TotalMilliseconds; gaps.Add(now-last); last=now; };
            GC.Collect(); GC.WaitForPendingFinalizers(); var managedBefore = GC.GetTotalMemory(true); heartbeat.Start();
                        for (var i=0;i<180;i++)
            {
                shell.Page = (ShellPage)(i%3);
                if (shell.Page == ShellPage.Home) shell.Launch.Select(shell.Library.Cards[i/3%3].Instance.Id);
                if (i%12==0) shell.Settings.ThemeIndex = i%24==0 ? 1 : 2;
                await Task.Delay(40);
            }
            heartbeat.Stop(); shell.Page = ShellPage.Home; shell.Settings.ThemeIndex = 1; await Task.Delay(500);
            GC.Collect(); GC.WaitForPendingFinalizers(); var managedAfter=GC.GetTotalMemory(true);
            Check(managedAfter-managedBefore < 16*1024*1024, "Stress memory growth bounded in observed run");
            process.Refresh(); var idleCpu=process.TotalProcessorTime; await Task.Delay(2000); process.Refresh(); var idleMs=(process.TotalProcessorTime-idleCpu).TotalMilliseconds;
            gaps.Sort();
            var outcome = new { Passed=true, Checks=checks, Frames=frames, NavigationTrace=navigationTrace, ManagedBefore=managedBefore, ManagedAfter=managedAfter, PrivateBytes=process.PrivateMemorySize64, IdleCpuMillisecondsPer2Seconds=idleMs, UiHeartbeatMaxMs=gaps.DefaultIfEmpty().Max(), UiHeartbeatP95Ms=gaps.Count==0?0:gaps[(int)(gaps.Count*.95)], ElapsedSeconds=watch.Elapsed.TotalSeconds, Dpi="100% actual; 125/150% render only. Not physically verified.", Capture="Running Avalonia visual tree, not a desktop screenshot.", InputVerification="Control bindings, focus and hover pseudoclass states; native-input evidence recorded separately." };
            File.WriteAllText(Path.Combine(folder,"result.json"),JsonSerializer.Serialize(outcome));
            _ = shell.Launch.StartAsync(); Button("关闭").RaiseEvent(new RoutedEventArgs(Avalonia.Controls.Button.ClickEvent));
        }
        catch(Exception error)
        {
            File.WriteAllText(Path.Combine(folder,"result.json"),JsonSerializer.Serialize(new {Passed=false,Checks=checks,Frames=frames,NavigationTrace=navigationTrace,Error=error.ToString()})); window.Close();
        }
        finally { shell.PropertyChanged -= trace; }
        return true;
    }
}
