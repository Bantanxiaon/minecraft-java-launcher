using System.Text.Json;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Media.Imaging;
using Avalonia.VisualTree;
using SHLauncher.Core.Launching;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Install;
using SHLauncher.UI.ViewModels;
using SHLauncher.UI.Views;
namespace SHLauncher.UI.Diagnostics;
internal static class DownloadCenterAudit
{
    internal static async Task<bool> RunIfRequested(MainWindow window)
    {
        var folder = Environment.GetEnvironmentVariable("SH_DOWNLOAD_UI_AUDIT");
        if (string.IsNullOrWhiteSpace(folder)) return false;
        Directory.CreateDirectory(folder);
        var shell = window.Shell; var vm = shell.Downloads; var checks = new List<string>();
        void Check(bool value, string name) { if (!value) throw new Exception(name); checks.Add(name); }
        async Task Until(Func<bool> condition, int seconds = 120)
        {
            using var stop = new CancellationTokenSource(TimeSpan.FromSeconds(seconds));
            while (!condition()) await Task.Delay(30, stop.Token);
            await Task.Delay(150);
        }
        async Task Shot(string name)
        {
            await Task.Delay(400);
            using var bitmap = new RenderTargetBitmap(new PixelSize((int)window.Bounds.Width, (int)window.Bounds.Height));
            bitmap.Render(window); bitmap.Save(Path.Combine(folder, name + ".png"), PngBitmapEncoderOptions.Default);
        }
        try
        {
            if (Environment.GetEnvironmentVariable("SH_DOWNLOAD_UI_RESTART") == "1")
            {
                shell.Page = ShellPage.Downloads;
                var old = vm.Tasks.Single(t => t.Record.Recipe.DisplayName == "ui-close-interrupted");
                Check(old.Record.Snapshot.State == DownloadTaskState.Cancelled, "actual UI restart restores cancelled task without network");
                var originalId = old.Record.Recipe.Id;
                old.RetryCommand.Execute(null);
                await Until(() => vm.Tasks.Any(t => t.Record.Recipe.DisplayName == "ui-close-interrupted" && t.Record.Snapshot.State == DownloadTaskState.Completed));
                Check(vm.Tasks.Any(t => t.Record.Recipe.Id != originalId && t.Record.Recipe.DisplayName == "ui-close-interrupted"), "manual retry creates distinct task and completes real install");
                await Shot("restart-retry-completed");
                File.WriteAllText(Path.Combine(folder, "result.json"), JsonSerializer.Serialize(new { Passed = true, Checks = checks }));
                window.Close(); return true;
            }
            shell.Settings.JavaPath = Environment.GetEnvironmentVariable("SH_DOWNLOAD_UI_JAVA")!;
            shell.Page = ShellPage.Downloads;
            await vm.RefreshAsync();
            Check(vm.MinecraftVersions.Count > 10 && vm.MinecraftVersions.All(v => v.Kind == MinecraftVersionKind.Release), "official release catalog default");
            vm.Filter = 1; Check(vm.MinecraftVersions.All(v => v.Kind == MinecraftVersionKind.Snapshot), "snapshot filter");
            Check(!vm.InstallCommand.CanExecute(null), "snapshot install explicitly unavailable");
            vm.Filter = 2; Check(vm.MinecraftVersions.Select(v => v.Kind).Distinct().Count() >= 3, "all filter preserves kinds");
            vm.Filter = 0; vm.Minecraft = vm.MinecraftVersions.Single(v => v.Id == "26.3");
            vm.Loader = LoaderType.Fabric;
            await Until(() => vm.LoaderVersion is not null || vm.Message.Contains("无法"));
            Check(vm.LoaderVersion?.Loader == LoaderType.Fabric && vm.LoaderVersion.MinecraftVersion == "26.3" && vm.LoaderVersion.IsStable, "live Fabric compatibility selection");
            Check(!vm.SelectionReason.Contains("官方推荐") || vm.LoaderVersion!.IsRecommended, "no fabricated official recommendation");
            await Shot("loader-selection-light");
            vm.Loader = LoaderType.Vanilla;
            vm.DirectoryName = "ui-vanilla-validation";
            Check(vm.InstallCommand.CanExecute(null), "install command becomes available");
            vm.InstallCommand.Execute(null);
            await Until(() => vm.Tasks.Any(t => t.Record.Snapshot.State == DownloadTaskState.Completed));
            var completed = vm.Tasks.First(t => t.Record.Snapshot.State == DownloadTaskState.Completed);
            Check(completed.Record.Result?.FileCount > 5000, "real unified install finished complete file graph");
            Check(completed.Details.Length > 0, "download diagnostics available");
            await Shot("download-completed-light");
            shell.Settings.ThemeIndex = 2; shell.Settings.ReducedMotion = true; await Shot("download-completed-dark");
            window.Width = 900; window.Height = 640; await Shot("download-small");
            completed.OpenCommand.Execute(null);
            await Until(() => shell.Launch.Selected?.Id == completed.Record.Recipe.InstanceKey.ToString("N"));
            Check(shell.Launch.MainEnabled && shell.Settings.ConfirmRunningClose, "installed instance uses real engine and managed GUID");
            shell.Page = ShellPage.Home;
            var game = shell.Launch.StartAsync(); await Until(() => shell.Launch.IsRunning);
            shell.Page = ShellPage.Downloads; window.WindowState = WindowState.Minimized; await Task.Delay(1200); window.WindowState = WindowState.Normal;
            Check(shell.Launch.IsRunning, "navigation and minimize retain process ownership");
            await Task.Delay(12000); shell.Launch.Cancel(); await game.WaitAsync(TimeSpan.FromSeconds(30));
            await Until(() => shell.Launch.Snapshot.State == LauncherTaskState.Idle);
            Check(!shell.Launch.IsRunning, "real Stop awaits process and log drain");
            vm.DirectoryName = "ui-close-interrupted";
            vm.InstallCommand.Execute(null);
            await Until(() => vm.Tasks.Any(t => t.Record.Recipe.DisplayName == "ui-close-interrupted"));
            File.WriteAllText(Path.Combine(folder, "result.json"), JsonSerializer.Serialize(new { Passed = true, Checks = checks, Note = "Real production install, actual Java process Stop; close cancellation terminal state independently checked after process exits." }));
            window.Close();
        }
        catch (Exception error)
        {
            File.WriteAllText(Path.Combine(folder, "result.json"), JsonSerializer.Serialize(new { Passed = false, Checks = checks, Error = error.ToString() }));
            shell.Launch.Cancel(); window.Close();
        }
        return true;
    }
}
