using System.Text.Json;
using SHLauncher.Contracts.Platform;
using SHLauncher.Core.Launching;
using SHLauncher.UI.Views;
namespace SHLauncher.UI.Diagnostics;
internal static class AccountGameAudit
{
    internal static async Task<bool> RunIfRequested(MainWindow window)
    {
        var output=Environment.GetEnvironmentVariable("SH_ACCOUNT_GAME_AUDIT");if(string.IsNullOrWhiteSpace(output))return false;
        Directory.CreateDirectory(output);var shell=window.Shell;var accounts=shell.Startup.Accounts;var checks=new List<string>();
        try
        {
            var restored=shell.Startup.Screen==SHLauncher.UI.ViewModels.RootScreen.MainShell;
            var expectRestore=Environment.GetEnvironmentVariable("SH_ACCOUNT_GAME_RESTORE")=="1";
            if(expectRestore&&!restored)throw new Exception("Expected active offline identity to restore without login gate");
            if(!restored){accounts.OfflineName="SHLNLocal";accounts.OfflineLoginCommand.Execute(null);await accounts.OfflineLoginCommand.DrainAsync();checks.Add("created offline account through production account command");}
            else checks.Add("restored active offline account without login gate");
            var root=Environment.GetEnvironmentVariable("SH_ACCOUNT_GAME_ROOT")!;var java=Environment.GetEnvironmentVariable("SH_ACCOUNT_GAME_JAVA")!;
            shell.Settings.JavaPath=java;shell.Settings.AttachDialogs(new Dialogs(root,java));shell.Settings.ChooseRootCommand.Execute(null);await shell.Settings.ChooseRootCommand.DrainAsync();
            if(shell.Launch.Instances.Count!=1||!shell.Launch.Instances[0].IsLaunchable)throw new Exception("Expected exactly one complete copied known-good instance");
            await Task.Delay(300);
            var account=accounts.Accounts.Single(a=>a.ProfileName=="SHLNLocal");
            await File.WriteAllTextAsync(Path.Combine(output,"ready.json"),JsonSerializer.Serialize(new{Ready=true,Restored=restored,account.ProfileName,account.ProfileId,account.AccountId,Game=shell.Launch.Instances[0].Id,Window=window.TryGetPlatformHandle()!.Handle.ToInt64()}));
            using var timeout=new CancellationTokenSource(TimeSpan.FromMinutes(10));
            while(shell.Launch.Snapshot.State is LauncherTaskState.Idle)await Task.Delay(100,timeout.Token);
            while(shell.Launch.Snapshot.State is LauncherTaskState.Preparing or LauncherTaskState.Launching)await Task.Delay(100,timeout.Token);
            if(shell.Launch.Snapshot.State!=LauncherTaskState.Running)throw new Exception("Game failed before Running: "+shell.Launch.Message);
            checks.Add("production Play command reached real running process");
            while(shell.Launch.Snapshot.State is LauncherTaskState.Running or LauncherTaskState.Cancelling)await Task.Delay(100,timeout.Token);
            if(shell.Launch.Snapshot.State!=LauncherTaskState.Idle)throw new Exception("Game did not exit normally: "+shell.Launch.Message);
            checks.Add("normal game exit returns coordinator to Idle");
            await File.WriteAllTextAsync(Path.Combine(output,"result.json"),JsonSerializer.Serialize(new{Passed=true,Checks=checks,account.ProfileName,account.ProfileId,account.AccountId,Restored=restored,Snapshot=shell.Launch.Snapshot}));
        }
        catch(Exception error){await File.WriteAllTextAsync(Path.Combine(output,"result.json"),JsonSerializer.Serialize(new{Passed=false,Checks=checks,Failure=error.ToString(),Snapshot=shell.Launch.Snapshot}));}
        window.Close();return true;
    }
    private sealed class Dialogs(string root,string java):IFileDialogService
    {
        public Task<string?> PickFolderAsync(CancellationToken token)=>Task.FromResult<string?>(root);
        public Task<string?> PickJavaAsync(CancellationToken token)=>Task.FromResult<string?>(java);
    }
}
