using System.Text.Json;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Media.Imaging;
using Avalonia.VisualTree;
using SHLauncher.UI.Shell;
namespace SHLauncher.UI.Diagnostics;
internal static class MasterAccountAudit
{
    public static async Task RunIfRequested(SHLNWindow window,ApplicationPresentation presentation)
    {
        var output=Environment.GetEnvironmentVariable("SH_MASTER_ACCOUNT_AUDIT");
        if(string.IsNullOrEmpty(output))return;
        Directory.CreateDirectory(output);var checks=new List<string>();
        void Check(bool value,string name){if(!value)throw new Exception(name);checks.Add(name);}
        void Capture(string name){window.UpdateLayout();using var bmp=new RenderTargetBitmap(new PixelSize((int)window.Bounds.Width,(int)window.Bounds.Height));bmp.Render(window);bmp.Save(Path.Combine(output,name+".png"),PngBitmapEncoderOptions.Default);}
        try
        {
            if(Environment.GetEnvironmentVariable("SH_ONLINE_CATALOG_AUDIT")=="1")
            {
                await using var catalogOwner=new SHLauncher.Bootstrap.LauncherComposition(Path.Combine(output,"catalog-fixture"));
                var provider=catalogOwner.OnlineModProviders.Single(p=>p.Id=="Modrinth");
                var context=new SHLauncher.Contracts.Content.InstanceCompatibilityContext("1.20.1","Fabric",null);
                var hits=await provider.SearchAsync(new("fabric api",SHLauncher.Contracts.Content.ContentType.Mod,"1.20.1","Fabric"),default);
                Check(hits.Count>0,"real Modrinth search through production DownloadManager");
                var selected=hits.FirstOrDefault(h=>h.Name.Equals("Fabric API",StringComparison.OrdinalIgnoreCase))??hits[0];
                var versions=await provider.VersionsAsync(selected.Id,context,default);
                Check(versions.Count>0&&versions.All(v=>v.Version.MinecraftVersions.Contains("1.20.1")&&v.Version.Loaders.Contains("fabric")),"real provider versions preserve Minecraft and Loader context");
                var exact=await provider.ExactVersionAsync(selected.Id,versions[0].Version.VersionId,default);
                Check(exact.Sha1==versions[0].Sha1&&exact.Size==versions[0].Size,"real exact version identity and hash match catalog");
                await OnlineModUiAudit.Run(window,output,catalogOwner.OnlineModProviders,Check,Capture);
                await File.WriteAllTextAsync(Path.Combine(output,"online-catalog.json"),JsonSerializer.Serialize(new{Provider=provider.Id,Project=selected,Versions=versions.Count,File=exact.FileName,exact.Size,exact.Sha1,GameFilesDownloaded=false},new JsonSerializerOptions{WriteIndented=true}));
            }
            var model=presentation.Model;var accounts=model.Startup.Accounts;var handle=window.TryGetPlatformHandle()!.Handle;var scene=window.Scene;
            accounts.OfflineName="MasterGate";accounts.OfflineLoginCommand.Execute(null);await accounts.OfflineLoginCommand.DrainAsync();await Task.Delay(300);
            var center=window.GetVisualDescendants().OfType<AccountCenter>().Single();
            Check(center.IsVisible,"account center visible after real offline login");
            var button=center.GetVisualDescendants().OfType<Button>().First();
            button.Flyout!.ShowAt(button);await Task.Delay(300);Capture("account-menu");button.Flyout.Hide();
            button.Flyout.ShowAt(button);await Task.Delay(100);
            var accountMenu=(StackPanel)((Flyout)button.Flyout).Content!;
            accountMenu.Children.OfType<Button>().Single(b=>Equals(b.Content,"切换账户")).RaiseEvent(new Avalonia.Interactivity.RoutedEventArgs(Button.ClickEvent));
            Check(accountMenu.Children.OfType<Button>().Any(b=>Equals(b.Content,"MasterGate")),"account switch menu lists real stored identities");
            accountMenu.Children.OfType<Button>().First(b=>Equals(b.Content,"MasterGate")).RaiseEvent(new Avalonia.Interactivity.RoutedEventArgs(Button.ClickEvent));await accounts.ActivateCommand.DrainAsync();
            Check(accounts.ActiveName=="MasterGate","account switch delegates to original activation");
            button.Flyout.ShowAt(button);await Task.Delay(100);
            accountMenu.Children.OfType<Button>().Single(b=>Equals(b.Content,"皮肤与披风")).RaiseEvent(new Avalonia.Interactivity.RoutedEventArgs(Button.ClickEvent));await Task.Delay(200);
            Check(window.GetVisualDescendants().OfType<CapePreview>().Any(),"skin and cape sheet renders capability-aware preview");Capture("skin-cape");
            window.GetVisualDescendants().OfType<Ursa.Controls.OverlayShared.OverlayFeedbackElement>().Last().Close();await Task.Delay(100);
            model.Page=SHLauncher.UI.ViewModels.ShellPage.Settings;await Task.Delay(200);Capture("settings-dark");
            model.Page=SHLauncher.UI.ViewModels.ShellPage.Home;await Task.Delay(100);
            foreach(var width in new[]{800,1100,1440}){window.Width=width;await Task.Delay(200);Capture("home-"+width);var pos=center.TranslatePoint(default,window)!.Value;Check(pos.Y<48&&pos.X>100&&pos.X+center.Bounds.Width<=width-130,"account titlebar bounds "+width);}
            for(var i=0;i<10;i++){model.Startup.ManageAccountsCommand.Execute(null);await Task.Delay(30);model.Startup.ReturnCommand.Execute(null);await Task.Delay(30);}
            Check(window.TryGetPlatformHandle()!.Handle==handle&&ReferenceEquals(scene,window.Scene),"account navigation preserves same native window and scene");
            accounts.SelectedAccount=accounts.ActiveAccount;accounts.LogoutCommand.Execute(null);await accounts.LogoutCommand.DrainAsync();
            Check(!accounts.HasActive,"real logout clears active account");
            model.Startup.ManageAccountsCommand.Execute(null);await Task.Delay(100);Check(!center.IsVisible,"no active account has no stale center");
            accounts.SelectedAccount=accounts.Accounts.First(a=>a.ProfileName=="MasterGate");accounts.ActivateCommand.Execute(null);await accounts.ActivateCommand.DrainAsync();await Task.Delay(100);
            Check(accounts.HasActive&&center.IsVisible,"original activation restores center");
            model.Page=SHLauncher.UI.ViewModels.ShellPage.Downloads;await Task.Delay(300);
            await model.Downloads.RefreshCommand.DrainAsync();
            for(var wait=0;wait<200&&model.Downloads.Busy;wait++)await Task.Delay(100);
            Check(!model.Backgrounds.IsPlaying,"workspace pauses obscured starfield");Capture("downloads");
            Check(model.Downloads.MinecraftVersions.Count>0,"download workspace uses real version catalog");
            var install=window.GetVisualDescendants().OfType<Button>().Single(b=>b.Content is string text&&text=="安装所选版本");
            install.RaiseEvent(new Avalonia.Interactivity.RoutedEventArgs(Button.ClickEvent));await Task.Delay(300);
            Check(window.GetVisualDescendants().OfType<SHLauncher.UI.Views.Pages.InstallOptions>().Any(),"version selection opens install sheet with original model");Capture("install-sheet");
            window.GetVisualDescendants().OfType<Ursa.Controls.OverlayShared.OverlayFeedbackElement>().Last().Close();await Task.Delay(300);
            model.Page=SHLauncher.UI.ViewModels.ShellPage.Mods;await Task.Delay(100);
            await ModWorkspaceAudit.Run(window,output,Check,Capture);
            model.Page=SHLauncher.UI.ViewModels.ShellPage.Home;await Task.Delay(200);Check(model.Backgrounds.IsPlaying,"Home resumes same shared scene");
            Check(presentation.Tray.Available,"native tray menu exporter available");
            var traySettings=presentation.Tray.Settings;var closeMode=traySettings.GetVisualDescendants().OfType<ComboBox>().Single();closeMode.SelectedIndex=2;
            traySettings.GetVisualDescendants().OfType<Button>().Single(b=>b.Content is string label&&label=="保存关闭方式").RaiseEvent(new Avalonia.Interactivity.RoutedEventArgs(Button.ClickEvent));
            for(var cycle=0;cycle<20;cycle++)
            {
                window.Close();for(var wait=0;wait<100&&window.IsVisible;wait++)await Task.Delay(20);
                Check(!window.IsVisible&&!model.Backgrounds.IsPlaying,"tray hide pauses scene "+cycle);
                await presentation.Tray.CleanupAsync();presentation.Tray.Restore();await Task.Delay(40);
                Check(window.IsVisible&&window.TryGetPlatformHandle()!.Handle==handle&&ReferenceEquals(scene,window.Scene),"tray restore keeps same window and scene "+cycle);
            }
            Check(model.Backgrounds.Ready&&model.Backgrounds.IsPlaying,"background decoder resumes after inactive resource cleanup");
            await File.WriteAllTextAsync(Path.Combine(output,"result.json"),JsonSerializer.Serialize(new {Passed=true,Checks=checks},new JsonSerializerOptions{WriteIndented=true}));
        }
        catch(Exception e){await File.WriteAllTextAsync(Path.Combine(output,"result.json"),JsonSerializer.Serialize(new{Passed=false,Checks=checks,Error=e.ToString()}));}
        presentation.RequestExit();
    }
}
