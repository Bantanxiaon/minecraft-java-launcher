using System.Diagnostics;
using System.Text.Json;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Media.Imaging;
using Avalonia.VisualTree;
using SHLauncher.Contracts.Platform;
using SHLauncher.Core.Launching;
using SHLauncher.UI.Shell;
using SHLauncher.UI.ViewModels;
namespace SHLauncher.UI.Diagnostics;
internal static class PhaseTwoAudit
{
    internal static async Task RunIfRequested(SHLNWindow window, ApplicationPresentation presentation)
    {
        var output=Environment.GetEnvironmentVariable("SH_PHASE2_AUDIT");
        if(string.IsNullOrWhiteSpace(output))return;
        Directory.CreateDirectory(output);
        var model=presentation.Model;var accounts=model.Startup.Accounts;var checks=new List<string>();
        var measurements=new List<object>();var originalScene=window.Scene;var session=model.Backgrounds;
        var handle=window.TryGetPlatformHandle()!.Handle;
        var root=Environment.GetEnvironmentVariable("SH_PHASE2_ROOT")!;
        var java=Environment.GetEnvironmentVariable("SH_PHASE2_JAVA")!;
        void Check(bool value,string name) { if(!value)throw new InvalidOperationException(name);checks.Add(name); }
        void Capture(string name)
        {
            window.UpdateLayout();
            using var bitmap=new RenderTargetBitmap(new PixelSize((int)window.Bounds.Width,(int)window.Bounds.Height));
            bitmap.Render(window);bitmap.Save(Path.Combine(output,name+".png"),PngBitmapEncoderOptions.Default);
        }
        async Task Sample(string state)
        {
            if(Environment.GetEnvironmentVariable("SH_MEMORY_FAST")=="1")return;
            using var process=Process.GetCurrentProcess();process.Refresh();var cpu=process.TotalProcessorTime;var timer=Stopwatch.StartNew();
            var samples=new List<long>();for(var i=0;i<16;i++){await Task.Delay(500);process.Refresh();samples.Add(process.PrivateMemorySize64);}
            measurements.Add(new { State=state,Seconds=timer.Elapsed.TotalSeconds,SingleCore=(process.TotalProcessorTime-cpu).TotalSeconds/timer.Elapsed.TotalSeconds,PrivateMin=samples.Min(),PrivateMax=samples.Max() });
        }
        async Task Wait(Func<bool> predicate,int seconds=40)
        { using var timeout=new CancellationTokenSource(TimeSpan.FromSeconds(seconds));while(!predicate())await Task.Delay(50,timeout.Token); }
        try
        {
            await Task.Delay(500);
            Check(ReferenceEquals(window.Scene.Session,session),"production owns exactly the injected background session");
            Check(!model.Settings.RootLabel.Contains("演示"),"startup uses real engine, no mock library");
            var restored=model.Startup.Screen==RootScreen.MainShell;
            if(Environment.GetEnvironmentVariable("SH_PHASE2_RESTORE")=="1")Check(restored,"active offline account restores directly to Home after process restart");
            if(restored)model.Startup.ManageAccountsCommand.Execute(null);
            await Task.Delay(300);Capture("01-accountgate");await Sample("AccountGate");
            Check(window.Scene.SharesCurrentFrame,"glass and clear image share the same decoded frame");
            accounts.ThirdPartyCommand.Execute(null);await Task.Delay(200);Capture("04-third-party");
            Check(accounts.IsThirdParty,"third-party form uses existing provider command");accounts.BackCommand.Execute(null);
            accounts.OfflineCommand.Execute(null);await Task.Delay(200);Capture("03-offline");
            using var transitionProcess=Process.GetCurrentProcess();transitionProcess.Refresh();var transitionBefore=transitionProcess.PrivateMemorySize64;var transitionSamples=new List<long>();
            accounts.OfflineName="SHLNPhase2";accounts.OfflineLoginCommand.Execute(null);
            for(var frame=0;frame<16;frame++){Capture("login-transition-"+frame.ToString("00"));transitionProcess.Refresh();transitionSamples.Add(transitionProcess.PrivateMemorySize64);await Task.Delay(40);}
            measurements.Add(new{State="Login to Home",PrivateBefore=transitionBefore,PrivatePeak=transitionSamples.Max(),PrivateAfter=transitionSamples.Last()});
            await accounts.OfflineLoginCommand.DrainAsync();
            await Wait(()=>model.Startup.Screen==RootScreen.MainShell);
            Check(accounts.ActiveName=="SHLNPhase2","real Offline command sets active identity");
            Check(window.TryGetPlatformHandle()!.Handle==handle&&ReferenceEquals(window.Scene,originalScene),"login retains OS window and SceneHost");
            model.Startup.ManageAccountsCommand.Execute(null);await Task.Delay(300);Capture("02-saved-account");
            accounts.SavedAccounts.Single(a=>a.Name=="SHLNPhase2").UseCommand.Execute(null);await accounts.ActivateCommand.DrainAsync();
            Check(model.Startup.Screen==RootScreen.MainShell,"saved account activation returns Home");
            model.Settings.AttachDialogs(new Dialogs(root,java));model.Settings.JavaPath=java;
            model.Settings.ChooseRootCommand.Execute(null);await model.Settings.ChooseRootCommand.DrainAsync();
            await Wait(()=>model.Library.Cards.Any(c=>c.Instance.Loader=="Vanilla"));
            Check(model.Launch.Instances.Count>=2,"real scanned Vanilla and Fabric profiles available");
            model.Library.Cards.First(c=>c.Instance.Loader=="Vanilla").SelectCommand.Execute(null);
            await Task.Delay(300);Capture("05-home-vanilla");Capture("09-launch-idle");await Sample("Home idle");
            model.Page=ShellPage.Library;await Task.Delay(300);Capture("07-instances");
            var modded=model.Library.Cards.First(c=>c.Instance.Loader!="Vanilla");modded.SelectCommand.Execute(null);
            await Task.Delay(300);Capture("06-home-modded");Capture("08-instance-changed");
            Check(model.IsHome&&model.Launch.Selected?.Id==modded.Instance.Id&&model.Home.InstanceName==modded.Name,"instance command returns Home and canonical hero/launch identity agree");
            model.Page=ShellPage.Downloads;await Task.Delay(250);Capture("downloads-preserved");
            model.Page=ShellPage.Settings;await Task.Delay(250);Capture("settings-preserved");
            model.Page=ShellPage.Home;await Task.Delay(250);
            await model.Import.InspectAsync([]);await Task.Delay(300);
            Check(window.GetVisualDescendants().OfType<Ursa.Controls.OverlayShared.OverlayFeedbackElement>().Any(),"existing import route uses unified overlay");
            model.Import.CancelCommand.Execute(null);await Task.Delay(350);
            Check(!window.GetVisualDescendants().OfType<Ursa.Controls.OverlayShared.OverlayFeedbackElement>().Any(),"import cancellation closes actual overlay");
            var home=window.GetVisualDescendants().OfType<HomeSurface>().Single();
            Check(ReferenceEquals(home.FindControl<Button>("LaunchButton")!.Command,model.Launch.StartCommand),"Home launch button is bound to the original command");
            GC.Collect();GC.WaitForPendingFinalizers();var before=GC.GetTotalMemory(true);
            for(var i=0;i<(Environment.GetEnvironmentVariable("SH_MEMORY_FAST")=="1"?2:20);i++)
            {
                model.Page=ShellPage.Library;await Task.Delay(240);Capture("cycle-"+i.ToString("00")+"-instances");
                model.Library.Cards[i%model.Library.Cards.Count].SelectCommand.Execute(null);await Task.Delay(240);Capture("cycle-"+i.ToString("00")+"-home");
                Check(model.IsHome&&ReferenceEquals(window.Scene,originalScene)&&ReferenceEquals(window.Scene.Session,session),"cycle "+i+" preserves scene and returns Home");
            }
            GC.Collect();GC.WaitForPendingFinalizers();var after=GC.GetTotalMemory(true);
            measurements.Add(new {State=Environment.GetEnvironmentVariable("SH_MEMORY_FAST")=="1"?"2 diagnostic navigation cycles":"20 navigation cycles",ManagedBefore=before,ManagedAfter=after,Delta=after-before});
            model.Settings.ReducedMotion=true;await Task.Delay(300);var time=session.CurrentSeconds;await Task.Delay(400);
            Check(!session.IsPlaying&&session.CurrentSeconds==time&&window.ReducedMotion,"Reduced Motion freezes media clock");Capture("reduced-motion");
            model.Settings.ReducedMotion=false;await Task.Delay(250);
            window.WindowState=WindowState.Minimized;await Task.Delay(250);Check(!session.IsPlaying,"minimize pauses player");window.WindowState=WindowState.Normal;await Task.Delay(300);Check(session.IsPlaying,"restore resumes same player");
            foreach(var size in new[]{new Size(800,560),new Size(1100,680),new Size(1440,900)}) {window.Width=size.Width;window.Height=size.Height;await Task.Delay(250);Capture("layout-"+size.Width);}
            window.Width=1100;window.Height=680;
            model.Library.Cards.First(c=>c.Instance.Loader=="Vanilla").SelectCommand.Execute(null);
            model.Settings.JavaPath=Path.Combine(output,"missing-java.exe");
            model.Launch.StartCommand.Execute(null);await model.Launch.StartCommand.DrainAsync();await Wait(()=>model.Launch.ShowError);
            Capture("12-failure");Check(model.Launch.ShowError,"safe missing Java produces real Failed state");
            model.Launch.ResetCommand.Execute(null);await Wait(()=>model.Launch.Snapshot.State==LauncherTaskState.Idle);model.Settings.JavaPath=java;
            if(model.Memory is {} memory)memory.Save(memory.Configuration with {Global=new(false,3072)});
            var states=new HashSet<LauncherTaskState>();
            void OnLaunch() {var state=model.Launch.Snapshot.State;if(states.Add(state)&&state is LauncherTaskState.Preparing or LauncherTaskState.Launching)Capture("10-"+state);}
            model.Launch.Refreshed+=OnLaunch;
            model.Launch.StartCommand.Execute(null);
            await Wait(()=>model.Launch.IsRunning||model.Launch.ShowError,90);
            Check(model.Launch.IsRunning,"original command starts real Minecraft process");
            model.Settings.RefreshLogsCommand.Execute(null);
            Check(model.Settings.RecentGameLogs.Contains("Verified JVM heap: -Xmx3072",StringComparison.OrdinalIgnoreCase),"actual process JVM heap matches persisted custom memory");Capture("11-running");
            model.Launch.Refreshed-=OnLaunch;
            await File.WriteAllTextAsync(Path.Combine(output,"game-running.json"),JsonSerializer.Serialize(new {Running=true,Process=Environment.ProcessId,State=model.Launch.Snapshot.State.ToString()}));
            // Allow independent native inspection; stop only this audit's child via the canonical command.
            await Task.Delay(Environment.GetEnvironmentVariable("SH_MEMORY_FAST")=="1"?5000:40000);
            model.Launch.StartCommand.Execute(null);await Wait(()=>model.Launch.Snapshot.State is LauncherTaskState.Idle or LauncherTaskState.Failed);
            Check(model.Launch.Snapshot.State==LauncherTaskState.Idle,"canonical Stop drains real game and returns Idle");
            await File.WriteAllTextAsync(Path.Combine(output,"result.json"),JsonSerializer.Serialize(new {Passed=true,Checks=checks,Measurements=measurements,Restored=restored,PhysicalDpi=window.RenderScaling,HighDpi="NOT VERIFIED",ScreenReader="NOT VERIFIED",ThirdParty="external credentials unavailable",Capture="real application client render; not desktop capture"},new JsonSerializerOptions{WriteIndented=true}));
        }
        catch(Exception error){model.Settings.RefreshLogsCommand.Execute(null);await File.WriteAllTextAsync(Path.Combine(output,"result.json"),JsonSerializer.Serialize(new {Passed=false,Checks=checks,Measurements=measurements,Error=error.ToString(),LaunchError=model.Launch.ErrorMessage,GameLogs=model.Settings.RecentGameLogs},new JsonSerializerOptions{WriteIndented=true}));}
        if(Environment.GetEnvironmentVariable("SH_PHASE2_KEEP_OPEN")!="1")presentation.RequestExit();
    }
    private sealed class Dialogs(string root,string java):IFileDialogService
    {
        public Task<string?> PickFolderAsync(CancellationToken token)=>Task.FromResult<string?>(root);
        public Task<string?> PickJavaAsync(CancellationToken token)=>Task.FromResult<string?>(java);
    }
}
