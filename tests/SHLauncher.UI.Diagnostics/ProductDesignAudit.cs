using System.Reflection;
using System.Text.Json;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Media.Imaging;
using Avalonia.VisualTree;
using SHLauncher.Application.Install;
using SHLauncher.Contracts.Install;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Core.Launching;
using SHLauncher.UI.ViewModels;
using SHLauncher.UI.Views;
namespace SHLauncher.UI.Diagnostics;
internal static class ProductDesignAudit
{
    internal static async Task<bool> RunIfRequested(MainWindow window)
    {
        var folder=Environment.GetEnvironmentVariable("SH_PRODUCT_DESIGN_AUDIT");
        if(string.IsNullOrWhiteSpace(folder))return false;
        Directory.CreateDirectory(folder);window.Title="SH Launcher Next · 视觉验收";
        var shell=window.Shell;var vm=shell.Downloads;var checks=new List<string>();var frames=new List<object>();
        var workspace=(IInstallationWorkspace)typeof(DownloadsViewModel).GetField("_workspace",BindingFlags.NonPublic|BindingFlags.Instance)!.GetValue(vm)!;
        var root=Environment.GetEnvironmentVariable("SH_DOWNLOAD_UI_WORKSPACE")!;
        void Check(bool value,string name){if(!value)throw new Exception(name);checks.Add(name);}
        async Task Until(Func<bool> value,int seconds=120){using var stop=new CancellationTokenSource(TimeSpan.FromSeconds(seconds));while(!value())await Task.Delay(10,stop.Token);}
        async Task Shot(string name,int delay=250)
        {
            await Task.Delay(delay);
            using var bitmap=new RenderTargetBitmap(new PixelSize((int)window.Bounds.Width,(int)window.Bounds.Height));
            bitmap.Render(window);bitmap.Save(Path.Combine(folder,name+".png"),PngBitmapEncoderOptions.Default);
            frames.Add(new{Name=name,window.ClientSize,Theme=window.ActualThemeVariant.ToString(),Launch=shell.Launch.Snapshot.State.ToString(),Active=vm.ActiveCount});
            foreach(var scroll in window.GetVisualDescendants().OfType<ScrollViewer>().Where(s=>s.IsEffectivelyVisible&&Avalonia.Automation.AutomationProperties.GetName(s)?.EndsWith("内容",StringComparison.Ordinal)==true))
                Check(scroll.Extent.Width<=scroll.Viewport.Width+1,name+" no horizontal overflow");
        }
        try
        {
            shell.Page=ShellPage.Downloads;shell.Settings.ThemeIndex=1;
            await Shot("downloads-idle-light");shell.Settings.ThemeIndex=2;await Shot("downloads-idle-dark");shell.Settings.ThemeIndex=1;
            foreach(var loader in Enum.GetValues<LoaderType>())
            {
                vm.Loader=loader;Check(!vm.LoaderDescription.Contains("Vanilla")&&(loader==LoaderType.Vanilla?vm.LoaderDescription.Contains("原版"):vm.LoaderDescription.Contains(loader.ToString())),"helper synchronizes immediately "+loader);
            }
            vm.Loader=LoaderType.Vanilla;await vm.RefreshAsync();
            Check(vm.MinecraftVersions.Count>10,"live release catalog");
            var catalog=vm.MinecraftVersions.ToArray();vm.Minecraft=vm.MinecraftVersions.Single(v=>v.Id=="1.20.1");vm.Loader=LoaderType.Forge;
            await Until(()=>vm.LoaderVersion is not null||vm.HasResolutionProblem);
            Check(vm.LoaderVersion?.Loader==LoaderType.Forge&&vm.LoaderVersion.MinecraftVersion=="1.20.1","live Forge identity without guessed recommendation");
            vm.DirectoryName="界面验收 Forge";Check(vm.InstallCommand.CanExecute(null)&&!vm.LoaderDescription.Contains("原版"),"Forge helper and install-ready state");
            await Shot("downloads-forge-light");shell.Settings.ThemeIndex=2;await Shot("downloads-forge-dark");shell.Settings.ThemeIndex=1;
            vm.Filter=1;Check(!vm.InstallCommand.CanExecute(null)&&vm.InstallBlocker.Contains("正式版"),"snapshot remains browse-only");
            await Shot("downloads-snapshot-blocked");vm.Filter=0;vm.Loader=LoaderType.Vanilla;
            var host=window.GetVisualDescendants().OfType<TransitioningContentControl>().Single(c=>c.Name=="PageHost");
            await using(var fault=new DownloadsViewModel(new ProductAuditFaultWorkspace(workspace,catalog)))
            {
                await fault.InitializeAsync();await fault.RefreshAsync();fault.Minecraft=fault.MinecraftVersions.Single(v=>v.Id=="1.20.1");fault.DirectoryName="兼容故障演示";fault.Loader=LoaderType.Forge;
                await Until(()=>fault.HasResolutionProblem);
                Check(!fault.InstallCommand.CanExecute(null)&&!fault.ResolutionMessage.Contains("IOException")&&fault.ResolutionDetails.Contains("IOException")&&!fault.LoaderDescription.Contains("原版"),"controlled metadata failure: friendly primary, exact diagnostics, no guess");
                host.SetCurrentValue(ContentControl.ContentProperty,fault);await Shot("downloads-compatibility-failure-fixture");
                host.SetCurrentValue(ContentControl.ContentProperty,vm);
            }
            shell.Page=ShellPage.Home;await Task.Delay(350);shell.Page=ShellPage.Downloads;await Task.Delay(350);
            Check(ReferenceEquals(host.Content,vm),"diagnostic fixture restores normal page binding");
            vm.Minecraft=vm.MinecraftVersions.Single(v=>v.Id=="26.3");vm.DirectoryName="界面验收原版";
            await Shot("downloads-ready-light");shell.Settings.ThemeIndex=2;await Shot("downloads-ready-dark");shell.Settings.ThemeIndex=1;
            foreach(var size in new[]{new Size(900,640),new Size(1280,720),new Size(1440,900),new Size(1920,1080),new Size(2560,1440)})
            {window.Width=size.Width;window.Height=size.Height;await Task.Delay(250);await Shot("downloads-size-"+(int)size.Width+"x"+(int)size.Height);}
            window.Width=1440;window.Height=900;await Task.Delay(300);window.WindowState=WindowState.Maximized;await Shot("downloads-maximized");window.WindowState=WindowState.Normal;await Task.Delay(350);
            window.Width=1280;window.Height=800;await Task.Delay(300);
            vm.InstallCommand.Execute(null);await Until(()=>vm.Tasks.Any(t=>t.IsActive&&t.Record.Snapshot.TotalFiles>1000));
            var active=vm.Tasks.First(t=>t.IsActive);await Shot("downloads-active-light",0);shell.Settings.ThemeIndex=2;await Shot("downloads-active-dark",0);
            var detail=window.GetVisualDescendants().OfType<Expander>().First(e=>e.IsEffectivelyVisible&&e.Header?.ToString()=="技术详情");detail.IsExpanded=true;await Shot("downloads-detail-dark",150);
            Check(shell.SidebarStatus=="正在下载","sidebar reflects actual production install");active.CancelCommand.Execute(null);await active.CancelCommand.DrainAsync();await Until(()=>active.IsCancelled);
            Check(!active.Transfer.Contains("MiB/s")&&(!Directory.Exists(Path.Combine(root,"instances"))||!Directory.EnumerateFiles(Path.Combine(root,"instances"),"instance.json",SearchOption.AllDirectories).Any()),"cancel removes live speed and publishes no partial instance");
            await Shot("downloads-cancelled-dark");shell.Settings.ThemeIndex=1;await Shot("downloads-cancelled-light");
            active.RetryCommand.Execute(null);await Until(()=>vm.Tasks.Any(t=>t.IsCompleted),180);
            var complete=vm.Tasks.First(t=>t.IsCompleted);Check(complete.Record.Result!.FileCount>5000,"real production retry completes verified Vanilla graph");await Shot("downloads-completed-light");shell.Settings.ThemeIndex=2;await Shot("downloads-completed-dark");
            await Until(()=>shell.Launch.Selected?.Id==complete.Record.Recipe.InstanceKey.ToString("N"));
            shell.Settings.JavaPath=Environment.GetEnvironmentVariable("SH_PRODUCT_DESIGN_JAVA")!;shell.Page=ShellPage.Home;shell.Settings.ThemeIndex=1;
            var game=shell.Launch.StartAsync();await Until(()=>shell.Launch.Snapshot.State is LauncherTaskState.Running or LauncherTaskState.Failed,45);
            Check(shell.Launch.IsRunning,"actual managed Java process Running");await Shot("real-running-light");
            window.WindowState=WindowState.Minimized;await Task.Delay(400);Check(shell.Launch.IsRunning,"minimize retains actual process ownership");window.WindowState=WindowState.Normal;await Task.Delay(400);
            shell.Launch.Cancel();await game.WaitAsync(TimeSpan.FromSeconds(30));Check(shell.Launch.Snapshot.State==LauncherTaskState.Idle,"real Stop drains");
            shell.Settings.SourceMode=1;shell.Settings.SourceMode=2;shell.Settings.SourceMode=0;await shell.Settings.SaveSourceCommand.DrainAsync();
            Check(workspace.SourcePreferences.Mode==DownloadSourceMode.Automatic&&shell.Settings.SourceStatus=="已自动保存","rapid source changes auto-save final value");
            shell.Page=ShellPage.Settings;shell.Settings.ThemeIndex=1;await Shot("settings-production-light");shell.Settings.ThemeIndex=2;await Shot("settings-production-dark");
            var advanced=window.GetVisualDescendants().OfType<Expander>().Single(e=>e.Header?.ToString()=="高级");advanced.IsExpanded=true;await Shot("settings-advanced-dark");
            shell.Settings.ThemeIndex=1;shell.Page=ShellPage.Library;await Shot("library-managed-light");
            await shell.Import.InspectAsync([Environment.GetEnvironmentVariable("SH_IMPORT_UI_ARCHIVE")!]);await Shot("import-production-preview-light");
            shell.Import.ConfirmCommand.Execute(null);await Until(()=>vm.Tasks.Any(t=>t.IsFailed));shell.Page=ShellPage.Downloads;
            await Shot("downloads-failed-light");shell.Settings.ThemeIndex=2;await Shot("downloads-failed-dark");
            File.WriteAllText(Path.Combine(folder,"result.json"),JsonSerializer.Serialize(new{Passed=true,Checks=checks,Frames=frames,Notes="Real production MC metadata/install/cache/Cancel/Retry/Java process Stop. Controlled compatibility failure uses explicit diagnostic fixture. Direct ZIP inspection is not physical drag. Running does not prove menu."}));
            window.Close();
        }
        catch(Exception error)
        {shell.Launch.Cancel();File.WriteAllText(Path.Combine(folder,"result.json"),JsonSerializer.Serialize(new{Passed=false,Checks=checks,Frames=frames,Error=error.ToString()}));window.Close();}
        return true;
    }
}
