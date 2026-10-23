using System.Text.Json;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Media.Imaging;
using Avalonia.VisualTree;
using SHLauncher.Core.Launching;
using SHLauncher.UI.Components;
using SHLauncher.UI.ViewModels;
using SHLauncher.UI.Views;
namespace SHLauncher.UI.Diagnostics;
internal static class PresentationSurfaceAudit
{
 internal static async Task Run(MainWindow window,string output)
 {
  var shell=window.Shell;var scene=shell.Backgrounds;var checks=new List<string>();
  var backdrop=window.FindControl<SceneBackground>("SharedBackdrop")!;
  Check(Grid.GetColumnSpan(backdrop)==2,"Home covers full window with the same scene");
  Check(scene.IsPlaying,"Home scene continues after login");
  shell.Page=ShellPage.Library;await Task.Delay(300);Check(!scene.IsPlaying,"non-scenic management page pauses player");
  var next=shell.Library.Cards.First(c=>c.Instance.Id!=shell.Launch.Selected?.Id);next.SelectCommand.Execute(null);await Task.Delay(300);
  Check(shell.IsHome&&shell.Home.InstanceName==next.Name,"instance selection returns Home and synchronizes title");
  Check(ReferenceEquals(scene,shell.Backgrounds)&&scene.IsPlaying,"page navigation retains one player");
  foreach(var size in new[]{new Size(900,640),new Size(1280,720),new Size(1440,900),new Size(1920,1080),new Size(2560,1440)})
  {
   window.Width=size.Width;window.Height=size.Height;await Task.Delay(250);window.UpdateLayout();
   Check(window.GetVisualDescendants().OfType<Button>().Where(b=>b.IsEffectivelyVisible&&Avalonia.Automation.AutomationProperties.GetName(b)=="主启动按钮").Any(),$"Play visible at {size}");
   using var image=new RenderTargetBitmap(new PixelSize((int)window.Bounds.Width,(int)window.Bounds.Height));image.Render(window);image.Save(Path.Combine(output,$"home-{size.Width}x{size.Height}.png"),PngBitmapEncoderOptions.Default);
  }
  window.Width=1280;window.Height=800;shell.Settings.ReducedMotion=true;await Task.Delay(120);Check(!scene.IsPlaying,"Home Reduced Motion uses static fallback");
  shell.Settings.ReducedMotion=false;await Task.Delay(150);
  if(shell.Launch.Snapshot.CanLaunch&&shell.Launch.Instances.All(i=>i.Id is "elements" or "create" or "vanilla"))
  {
   shell.Launch.StartCommand.Execute(null);
   using(var wait=new CancellationTokenSource(TimeSpan.FromSeconds(14))){while(!shell.Launch.IsRunning)await Task.Delay(25,wait.Token);}
   Check(shell.Launch.IsRunning,"original Play command reaches mock coordinator Running");shell.Launch.StartCommand.Execute(null);await shell.Launch.StartCommand.DrainAsync();
   using var stop=new CancellationTokenSource(TimeSpan.FromSeconds(10));while(shell.Launch.Snapshot.State!=LauncherTaskState.Idle)await Task.Delay(25,stop.Token);
   Check(!shell.Launch.IsRunning,"same Play command stops mock game");
  }
  await File.WriteAllTextAsync(Path.Combine(output,"presentation-result.json"),JsonSerializer.Serialize(new{Passed=true,Checks=checks,NativeDpi="NOT VERIFIED; these are client-size renders"}));
  void Check(bool value,string name){if(!value)throw new Exception(name);checks.Add(name);}
 }
}
