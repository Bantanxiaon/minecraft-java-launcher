using System.Diagnostics;
using System.Text.Json;
using Avalonia;
using Avalonia.Media.Imaging;
using Avalonia.Interactivity;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.VisualTree;
using SHLauncher.Core.Launching;
using SHLauncher.UI.ViewModels;
using SHLauncher.UI.Views;

namespace SHLauncher.UI.Diagnostics;

internal static class ShellAudit
{
    internal static async Task RunIfRequested(MainWindow window)
    {
        if(await AccountGameAudit.RunIfRequested(window))return;
        if(await AccountFlowAudit.RunIfRequested(window))return;
        if(new[]{"SH_IMPORT_UI_AUDIT","SH_UI_VISUAL_DIR","SH_REAL_UI_AUDIT","SH_DOWNLOAD_UI_AUDIT","SH_UI_AUDIT_DIR"}.Any(key=>!string.IsNullOrWhiteSpace(Environment.GetEnvironmentVariable(key))))
        {
            if(window.Shell.Startup.Screen!=RootScreen.MainShell){var account=window.Shell.Startup.Accounts;account.OfflineName="AuditPlayer";account.OfflineLoginCommand.Execute(null);await account.OfflineLoginCommand.DrainAsync();await Task.Delay(280);}
        }
        if(new[]{"SH_IMPORT_UI_AUDIT","SH_UI_VISUAL_DIR","SH_REAL_UI_AUDIT","SH_DOWNLOAD_UI_AUDIT"}.Any(key=>!string.IsNullOrWhiteSpace(Environment.GetEnvironmentVariable(key))))
            window.Title="SH Launcher Next · 自动验收";
        if (await ProductDesignAudit.RunIfRequested(window)) return;
        if (await ImportPreviewAudit.RunIfRequested(window)) return;
        if (await DownloadCenterAudit.RunIfRequested(window)) return;
        if (await RealFoundationAudit.RunIfRequested(window)) return;
        if (await VisualAudit.RunIfRequested(window)) return;
        var directory = Environment.GetEnvironmentVariable("SH_UI_AUDIT_DIR");
        if (string.IsNullOrWhiteSpace(directory)) return;
        Directory.CreateDirectory(directory);
        var stopwatch = Stopwatch.StartNew();
        var results = new List<string>();
        var number = 0;
        var shell = window.Shell;
        var process = Process.GetCurrentProcess();
        var baselineCpu = process.TotalProcessorTime;
        var idleCpuMilliseconds = 0d;
        try
        {
            void Check(bool condition, string name)
            { if (!condition) throw new InvalidOperationException(name); results.Add(name); }
            Button Button(string name) => window.GetVisualDescendants().OfType<Button>().First(x => AutomationProperties.GetName(x) == name);
            async Task Click(string name)
            {
                var button = Button(name);
                Check(button.IsEffectivelyEnabled && button.Command is not null, name + " command binding");
                button.Command!.Execute(button.CommandParameter); await Task.Delay(280);
            }
            async Task WaitState(LauncherTaskState state)
            {
                using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(12));
                while (shell.Launch.Snapshot.State != state) await Task.Delay(15, timeout.Token);
                await Task.Delay(80);
            }
            async Task Shot(string name)
            {
                await Task.Delay(320); number++;
                if (!name.StartsWith("render-dpi", StringComparison.Ordinal))
                {
                    var pixels = new PixelSize((int)(window.Bounds.Width * window.RenderScaling), (int)(window.Bounds.Height * window.RenderScaling));
                    using var frame = new RenderTargetBitmap(pixels, new Vector(96 * window.RenderScaling, 96 * window.RenderScaling));
                    frame.Render(window);
                    frame.Save(Path.Combine(directory, name + ".png"), PngBitmapEncoderOptions.Default);
                }
                process.Refresh();
                var stage = new { Id = number, Name = name, Pid = Environment.ProcessId, Scale = window.RenderScaling,
                    Width = window.ClientSize.Width, Height = window.ClientSize.Height, State = shell.Launch.Snapshot.State.ToString(),
                    Theme = window.ActualThemeVariant.ToString(), ManagedBytes = GC.GetTotalMemory(false), PrivateBytes = process.PrivateMemorySize64,
                    CpuMilliseconds = process.TotalProcessorTime.TotalMilliseconds, ElapsedMilliseconds = stopwatch.Elapsed.TotalMilliseconds };
                File.WriteAllText(Path.Combine(directory, $"stage-{number}.json"), JsonSerializer.Serialize(stage));
                using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(20));
                while (!File.Exists(Path.Combine(directory, $"ack-{number}")))
                    await Task.Delay(30, timeout.Token);
            }
            await Task.Delay(400); process.Refresh(); var idleStart = process.TotalProcessorTime;
            await Task.Delay(1000); process.Refresh(); idleCpuMilliseconds = (process.TotalProcessorTime - idleStart).TotalMilliseconds;
            shell.Settings.ThemeIndex = 1;
            await Shot("home-light");
            Check(window.GetVisualDescendants().OfType<TextBlock>().Any(x => x.Text == "元素觉醒"), "Home instance binding");
            shell.Settings.ThemeIndex = 2; await Shot("home-dark");
            await Click("资料库"); await Click("Create World");
            Check(shell.Launch.Selected?.Id == "create", "Library selection");
            await Shot("library");
            await Click("首页");
            Check(shell.Home.InstanceName == "Create World", "Hero synchronizes with selection");
            await Click("设置");
            var theme = window.GetVisualDescendants().OfType<ComboBox>().First(x => AutomationProperties.GetName(x) == "主题");
            theme.SelectedIndex = 1; await Task.Delay(80);
            Check(shell.Settings.ThemeIndex == 1, "Theme two-way binding");
            var motion = window.GetVisualDescendants().OfType<ToggleSwitch>().Single();
            motion.IsChecked = true; await Task.Delay(80);
            Check(shell.Settings.ReducedMotion, "Reduced motion two-way binding");
            var slider = window.GetVisualDescendants().OfType<Slider>().Single(); slider.Value = 10240;
            Check(shell.Settings.MemoryMb == 10240, "Mock slider binding");
            await Shot("settings");
            theme.SelectedIndex = 0; await Task.Delay(80); Check(shell.Settings.ThemeIndex == 0, "System theme selection");
            await Click("首页"); window.Width = 900; window.Height = 640; await Shot("small-window");
            Button("最大化或还原").RaiseEvent(new RoutedEventArgs(Avalonia.Controls.Button.ClickEvent)); await Shot("maximized");
            Check(window.WindowState == WindowState.Maximized, "Caption maximize button");
            Button("最大化或还原").RaiseEvent(new RoutedEventArgs(Avalonia.Controls.Button.ClickEvent)); await Task.Delay(350);
            Check(window.WindowState == WindowState.Normal, "Caption restore button");
            Button("最小化").RaiseEvent(new RoutedEventArgs(Avalonia.Controls.Button.ClickEvent)); await Task.Delay(250);
            Check(window.WindowState == WindowState.Minimized, "Caption minimize button");
            window.WindowState = WindowState.Normal;
            await Task.Delay(350); window.Width = 900; window.Height = 640; await Task.Delay(350);
            foreach (var scale in new[] { 1.25, 1.5 })
            {
                var size = new PixelSize((int)(window.Bounds.Width * scale), (int)(window.Bounds.Height * scale));
                using var bitmap = new RenderTargetBitmap(size, new Vector(96 * scale, 96 * scale));
                bitmap.Render(window);
                var name = scale == 1.25 ? "render-dpi125" : "render-dpi150";
                bitmap.Save(Path.Combine(directory, name + ".png"), PngBitmapEncoderOptions.Default);
                Check(bitmap.PixelSize == size, name + " pixel size"); await Shot(name);
            }
            window.Width = 1200; window.Height = 850; shell.Settings.ReducedMotion = false;
            var failureRun = Environment.GetEnvironmentVariable("SH_MOCK_SCENARIO") == "LaunchFailure";
            if (failureRun)
            {
                Button("开始或结束游戏").Command!.Execute(null);
                await WaitState(LauncherTaskState.Failed); await Shot("failed");
                Check(shell.Launch.ShowError && shell.Launch.MainEnabled, "Failure actions enabled");
                Button("开始或结束游戏").Command!.Execute(null);
                await WaitState(LauncherTaskState.Running); await Shot("retry-running");
                shell.Launch.Cancel(); await WaitState(LauncherTaskState.Idle);
            }
            else
            {
                var play = Button("开始或结束游戏"); play.Command!.Execute(null);
                for (var i = 0; i < 30; i++) play.Command.Execute(null);
                await WaitState(LauncherTaskState.Launching); await Shot("launching");
                await Click("资料库"); Check(!shell.Launch.Select("elements"), "Busy selection rejected across pages");
                shell.Launch.Cancel(); await WaitState(LauncherTaskState.Idle); await Shot("cancelled");
                await Click("首页"); Button("开始或结束游戏").Command!.Execute(null);
                await WaitState(LauncherTaskState.Running); await Shot("running");
                await WaitState(LauncherTaskState.Idle); await Shot("game-exited");
                Check(!shell.Launch.ShowProgress, "Natural exit clears progress");
                Button("开始或结束游戏").Command!.Execute(null); await WaitState(LauncherTaskState.Running);
                Button("开始或结束游戏").Command!.Execute(null); await WaitState(LauncherTaskState.Idle);
                Check(!shell.Launch.ShowProgress, "Stop clears progress only after task ends");
            }
            for (var i = 0; i < 6; i++)
            {
                shell.Page = ShellPage.Home; var run = shell.Launch.StartAsync();
                await Task.Delay(45); shell.Page = ShellPage.Settings; shell.Launch.Cancel(); await run;
            }
            shell.Page = ShellPage.Home; await Task.Delay(300);
            GC.Collect(); GC.WaitForPendingFinalizers(); var managedBefore = GC.GetTotalMemory(true);
            for (var i = 0; i < 45; i++) { shell.Page = (ShellPage)(i % 3); await Task.Delay(30); }
            shell.Page = ShellPage.Home; await Task.Delay(400);
            GC.Collect(); GC.WaitForPendingFinalizers(); var managedAfter = GC.GetTotalMemory(true);
            Check(managedAfter - managedBefore < 12 * 1024 * 1024, "Navigation memory growth bounded after warmup");
            process.Refresh();
            File.WriteAllText(Path.Combine(directory, "result.json"), JsonSerializer.Serialize(new { Passed = true, Checks = results,
                ManagedBefore = managedBefore, ManagedAfter = managedAfter, PrivateBytes = process.PrivateMemorySize64,
                IdleCpuMilliseconds = idleCpuMilliseconds, CpuMilliseconds = (process.TotalProcessorTime - baselineCpu).TotalMilliseconds, ElapsedMilliseconds = stopwatch.Elapsed.TotalMilliseconds,
                Note = "125/150 percent are RenderTargetBitmap high-DPI render checks; physical display scaling and multi-monitor behavior remain unverified." }));
            // Closing while preparation is active exercises the same close path as the caption button.
            _ = shell.Launch.StartAsync(); Button("关闭").RaiseEvent(new RoutedEventArgs(Avalonia.Controls.Button.ClickEvent));
        }
        catch (Exception error)
        {
            File.WriteAllText(Path.Combine(directory, "result.json"), JsonSerializer.Serialize(new { Passed = false, Checks = results, Error = error.ToString() }));
            window.Close();
        }
    }
}
