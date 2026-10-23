using System.Text.Json;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Media.Imaging;
using SHLauncher.Contracts.Engine;
using SHLauncher.Contracts.Platform;
using SHLauncher.Core.Launching;
using SHLauncher.UI.ViewModels;
using SHLauncher.UI.Views;
namespace SHLauncher.UI.Diagnostics;
internal static class RealFoundationAudit
{
    internal static async Task<bool> RunIfRequested(MainWindow window)
    {
        var folder = Environment.GetEnvironmentVariable("SH_REAL_UI_AUDIT");
        if (string.IsNullOrWhiteSpace(folder)) return false;
        Directory.CreateDirectory(folder); var checks = new List<string>(); var shell = window.Shell;
        void Check(bool condition, string label) { if (!condition) throw new Exception(label); checks.Add(label); }
        async Task Wait(Func<bool> condition)
        {
            using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(20));
            while (!condition()) await Task.Delay(15, timeout.Token);
            await Task.Delay(150);
        }
        void Shot(string name)
        {
            using var bitmap = new RenderTargetBitmap(new PixelSize((int)window.Bounds.Width, (int)window.Bounds.Height));
            bitmap.Render(window); bitmap.Save(Path.Combine(folder, name + ".png"), PngBitmapEncoderOptions.Default);
        }
        try
        {
            var root = Environment.GetEnvironmentVariable("SH_REAL_UI_ROOT")!;
            var java = Environment.GetEnvironmentVariable("SH_REAL_UI_JAVA")!;
            shell.Settings.AttachDialogs(new ScriptedDialogs(root, java));
            shell.Settings.ChooseJavaCommand.Execute(null); await Wait(() => shell.Settings.JavaPath == java);
            shell.Settings.ChooseRootCommand.Execute(null); await Wait(() => shell.Launch.Instances.Count == 13);
            Check(shell.Library.Cards.Count == 13 && shell.Launch.Selected?.Id == "12", "real catalog updates Library and Home through common coordinator");
            shell.Page = ShellPage.Library; await Task.Delay(200); Shot("real-library");
            shell.Launch.Select("Closing Song1.6.5"); await Task.Delay(100);
            Check(!shell.Launch.MainEnabled && shell.Launch.Selected?.Problem?.Code == LauncherErrorCode.MissingMetadata, "invalid instance disables Play and preserves mapped problem");
            shell.Launch.Select("12"); shell.Page = ShellPage.Home;
            var launch = shell.Launch.StartAsync();
            Check(!await shell.Launch.StartAsync(), "duplicate Play rejected for real adapter");
            Check(!shell.Launch.Select("26.2"), "active instance locked during preparation");
            shell.Page = ShellPage.Settings; window.WindowState = WindowState.Minimized; await Task.Delay(180); window.WindowState = WindowState.Normal;
            await launch.WaitAsync(TimeSpan.FromSeconds(20)); await Task.Delay(200);
            Check(shell.Launch.Snapshot.State == LauncherTaskState.Failed && shell.Launch.Snapshot.Error?.Code == LauncherErrorCode.GameFileMissing, "real asset-index mismatch mapped without auto repair/download");
            Check(shell.Launch.Snapshot.OperationId != Guid.Empty, "real preparation has launch session id");
            shell.Settings.RefreshLogsCommand.Execute(null); Check(shell.Settings.RecentGameLogs.Length > 0, "sanitized real preparation logs visible in Settings");
            Shot("real-file-error-settings"); shell.Page = ShellPage.Home; await Task.Delay(150); Shot("real-file-error-home");
            shell.Launch.ResetCommand.Execute(null);
            var retry = shell.Launch.StartAsync(); shell.Launch.Cancel(); await retry.WaitAsync(TimeSpan.FromSeconds(10));
            Check(shell.Launch.Snapshot.State == LauncherTaskState.Idle, "real preparation cancel waits for cleanup");
            shell.Settings.MockModeCommand.Execute(null); await Wait(() => shell.Launch.Instances.Count == 3);
            Check(shell.Library.Cards.Count == 3 && shell.Launch.Selected?.Id != "12", "real-to-mock catalog replacement clears stale cards");
            shell.Settings.ChooseRootCommand.Execute(null); await Wait(() => shell.Launch.Instances.Count == 13);
            Check(shell.Settings.ConfirmRunningClose, "real engine close policy configured");
            File.WriteAllText(Path.Combine(folder, "result.json"), JsonSerializer.Serialize(new { Passed = true, Checks = checks,
                Input = "Running native Avalonia window; scripted IFileDialogService selection, not an OS picker interaction", RealGameStarts = 0,
                Pending = "Complete Vanilla required for running Stop, close confirmation and three normal game exits" }));
            window.Close();
        }
        catch (Exception e)
        {
            File.WriteAllText(Path.Combine(folder, "result.json"), JsonSerializer.Serialize(new { Passed = false, Checks = checks, Error = e.ToString(), Snapshot = shell.Launch.Snapshot })); window.Close();
        }
        return true;
    }
    private sealed class ScriptedDialogs(string root, string java) : IFileDialogService
    {
        public Task<string?> PickFolderAsync(CancellationToken token) => Task.FromResult<string?>(root);
        public Task<string?> PickJavaAsync(CancellationToken token) => Task.FromResult<string?>(java);
    }
}
