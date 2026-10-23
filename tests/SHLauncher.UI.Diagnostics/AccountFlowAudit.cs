using System.Diagnostics;
using System.Text.Json;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Media.Imaging;
using Avalonia.VisualTree;
using SHLauncher.UI.Presentation;
using SHLauncher.UI.ViewModels;
using SHLauncher.UI.Views;
namespace SHLauncher.UI.Diagnostics;
internal static class AccountFlowAudit
{
    private static string? DirectoryPath=>Environment.GetEnvironmentVariable("SH_ACCOUNT_UI_AUDIT");
    internal static void CaptureStartup(MainWindow window)
    {
        if(string.IsNullOrWhiteSpace(DirectoryPath))return;
        Directory.CreateDirectory(DirectoryPath);var theme=window.Shell.Settings.ThemeIndex;
        window.Shell.Settings.ThemeIndex=1;Shot(window,"splash-light");window.Shell.Settings.ThemeIndex=2;Shot(window,"splash-dark");window.Shell.Settings.ThemeIndex=theme;
    }
    internal static async Task<bool> RunIfRequested(MainWindow window)
    {
        if(string.IsNullOrWhiteSpace(DirectoryPath))return false;
        Directory.CreateDirectory(DirectoryPath);var checks=new List<string>();var timings=new List<object>();var shell=window.Shell;var accounts=shell.Startup.Accounts;var handle=window.TryGetPlatformHandle()!.Handle;
        var host=window.FindControl<TransitioningContentControl>("RootHost")!;
        if(host.PageTransition is MeasuredTransition transition)transition.Completed+=(requested,elapsed,cancelled)=>timings.Add(new{RequestedMs=requested.TotalMilliseconds,ActualMs=elapsed.TotalMilliseconds,Cancelled=cancelled});
        try
        {
            window.Title="SHLN · 星空验收";
            await Task.Delay(300);
            Check(shell.Startup.Screen==RootScreen.AccountGate,"fresh workspace opens account gate");Check(window.Width==1000&&window.Height==600,"split login window 1000x600 remains smaller than main shell");
            await SceneBackgroundAudit.Run(window,DirectoryPath!);
            shell.Settings.ThemeIndex=1;await ShotAsync("gate-light");shell.Settings.ThemeIndex=2;await ShotAsync("gate-dark");
            accounts.OfflineCommand.Execute(null);accounts.OfflineName="bad name";await Run(accounts.OfflineLoginCommand);
            Check(accounts.HasMessage&&shell.Startup.Screen==RootScreen.AccountGate,"invalid offline name stays at gate");await ShotAsync("offline-error");
            accounts.OfflineName="LocalReviewOne";await ShotAsync("offline-form");await Run(accounts.OfflineLoginCommand);
            Check(shell.Startup.Screen==RootScreen.MainShell&&accounts.ActiveName=="LocalReviewOne","valid offline reaches shell");Check(window.Width==1280&&window.Height==800,"main shell restores larger geometry");
            await PresentationSurfaceAudit.Run(window,DirectoryPath!);
            await ShotAsync("login-success-dark");shell.Settings.ThemeIndex=1;await ShotAsync("login-success-light");
            shell.Startup.ManageAccountsCommand.Execute(null);await Task.Delay(280);accounts.BackCommand.Execute(null);accounts.OfflineCommand.Execute(null);accounts.OfflineName="LocalReviewTwo";await Run(accounts.OfflineLoginCommand);
            Check(accounts.Accounts.Count==2&&accounts.ActiveName=="LocalReviewTwo","second local identity");
            shell.Startup.ManageAccountsCommand.Execute(null);await Task.Delay(280);accounts.BackCommand.Execute(null);accounts.SelectedAccount=accounts.Accounts.Single(a=>a.ProfileName=="LocalReviewOne");await Run(accounts.ActivateCommand);
            Check(accounts.ActiveName=="LocalReviewOne","switch account");
            shell.Startup.ManageAccountsCommand.Execute(null);await Task.Delay(280);await ShotAsync("account-management");
            accounts.SelectedAccount=accounts.Accounts.Single(a=>a.ProfileName=="LocalReviewOne");await Run(accounts.LogoutCommand);Check(!accounts.HasActive,"logout removes active selection");await ShotAsync("logged-out");
            accounts.SelectedAccount=accounts.Accounts.Single(a=>a.ProfileName=="LocalReviewOne");await Run(accounts.RemoveCommand);Check(accounts.Accounts.Count==1,"remove account");
            accounts.ThirdPartyCommand.Execute(null);await ShotAsync("thirdparty-light");shell.Settings.ThemeIndex=2;await ShotAsync("thirdparty-dark");
            accounts.Identifier="";accounts.Password="";await Run(accounts.OnlineLoginCommand);Check(accounts.HasMessage&&accounts.Password.Length==0,"invalid thirdparty fields clear password");await ShotAsync("thirdparty-error");
            var password=window.GetVisualDescendants().OfType<TextBox>().Single(t=>t.Name=="PasswordInput");Check(password.PasswordChar!='\0',"password field is masked");
            accounts.BackCommand.Execute(null);accounts.SelectedAccount=accounts.Accounts.Single();await Run(accounts.ActivateCommand);
            shell.Settings.ReducedMotion=true;var clock=Stopwatch.StartNew();shell.Startup.ManageAccountsCommand.Execute(null);await Task.Delay(30);Check(shell.Startup.Screen==RootScreen.AccountGate,"reduced-motion state changes without waiting for animation");shell.Startup.ReturnCommand.Execute(null);await Task.Delay(30);
            Check(window.TryGetPlatformHandle()!.Handle==handle,"one stable native window across all root states");
            Check(timings.Any(t=>JsonSerializer.Serialize(t).Contains("\"RequestedMs\":0")),"zero duration root transition observed");
            await File.WriteAllTextAsync(Path.Combine(DirectoryPath!,"result.json"),JsonSerializer.Serialize(new{Passed=true,Checks=checks,Timings=timings,WindowHandleStable=true,ThirdPartyRealGame="NOT VERIFIED - no legitimate test account",Microsoft="Unavailable"}));
        }
        catch(Exception error)
        {await File.WriteAllTextAsync(Path.Combine(DirectoryPath!,"result.json"),JsonSerializer.Serialize(new{Passed=false,Checks=checks,Failure=error.ToString(),Timings=timings}));}
        window.Close();return true;
        void Check(bool condition,string name){if(!condition)throw new Exception(name);checks.Add(name);}
        async Task Run(AsyncCommand command){command.Execute(null);await command.DrainAsync();await Task.Delay(280);}
        async Task ShotAsync(string name){await Task.Delay(260);Shot(window,name);}
    }
    private static void Shot(MainWindow window,string name)
    {
        window.UpdateLayout();var pixels=new PixelSize(Math.Max(1,(int)(window.Bounds.Width*window.RenderScaling)),Math.Max(1,(int)(window.Bounds.Height*window.RenderScaling)));
        using var bitmap=new RenderTargetBitmap(pixels,new Vector(96*window.RenderScaling,96*window.RenderScaling));bitmap.Render(window);bitmap.Save(Path.Combine(DirectoryPath!,name+".png"),PngBitmapEncoderOptions.Default);
    }
}
