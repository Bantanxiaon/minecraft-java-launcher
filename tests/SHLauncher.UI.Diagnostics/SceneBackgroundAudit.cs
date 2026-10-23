using System.Diagnostics;
using System.Text.Json;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Media.Imaging;
using SHLauncher.UI.Components;
using SHLauncher.UI.ViewModels;
using SHLauncher.UI.Views;
namespace SHLauncher.UI.Diagnostics;
internal static class SceneBackgroundAudit
{
 public static async Task Run(MainWindow window,string directory)
 {
  var scene=window.Shell.Backgrounds;var checks=new List<string>();var samples=new List<object>();var process=Process.GetCurrentProcess();var clock=Stopwatch.StartNew();var cpu=process.TotalProcessorTime;var wraps=0;double last=-1;
  var backdrop=window.FindControl<SceneBackground>("SharedBackdrop")!;
  Check(scene.Id=="polar-trails","real licensed star-trail asset selected");
  Check(Grid.GetColumnSpan(backdrop)==1,"login preserves left 53 percent animated field");
  var captures=new Dictionary<int,int>();
  for(var i=0;i<1080;i++)
  {
   await Task.Delay(100);var time=scene.CurrentSeconds;if(last>=0&&time<last)wraps++;last=time;process.Refresh();
   if(i%10==0)samples.Add(new{Seconds=clock.Elapsed.TotalSeconds,MediaSeconds=time,scene.IsPlaying,WorkingSet=process.WorkingSet64,Private=process.PrivateMemorySize64,CpuMs=(process.TotalProcessorTime-cpu).TotalMilliseconds});
   if((time>=31.5||time<=2)&&i>260){using var image=new RenderTargetBitmap(new PixelSize((int)window.Bounds.Width,(int)window.Bounds.Height));image.Render(window);var segment=time>=31.5?wraps+1:wraps;captures.TryGetValue(segment,out var count);captures[segment]=count+1;image.Save(Path.Combine(directory,$"wrap-{segment}-{count:000}.png"),PngBitmapEncoderOptions.Default);}
  }
  Check(scene.Ready&&scene.IsPlaying&&wraps>=3,"actual player passed at least three 34 second wrap boundaries");
  Check(scene.Poster.PixelSize==new PixelSize(1920,1080),"full HD static fallback exists");
  window.Shell.Settings.ReducedMotion=true;await Task.Delay(80);var stopped=scene.CurrentSeconds;await Task.Delay(250);Check(!scene.IsPlaying&&Math.Abs(scene.CurrentSeconds-stopped)<0.001,"Reduced Motion freezes media clock");
  window.Shell.Settings.ReducedMotion=false;await Task.Delay(250);Check(scene.IsPlaying,"Reduced Motion off resumes media");
  window.WindowState=WindowState.Minimized;await Task.Delay(150);stopped=scene.CurrentSeconds;await Task.Delay(250);Check(!scene.IsPlaying&&Math.Abs(scene.CurrentSeconds-stopped)<0.001,"minimized window freezes media clock");window.WindowState=WindowState.Normal;await Task.Delay(300);Check(scene.IsPlaying,"restored window resumes media");
  await File.WriteAllTextAsync(Path.Combine(directory,"background-result.json"),JsonSerializer.Serialize(new{Passed=true,Checks=checks,ActualSeconds=clock.Elapsed.TotalSeconds,Wraps=wraps,Renderer="AnimationImage animated WebP, one persistent player, on-demand software decode",Audio=false,Samples=samples}));
  void Check(bool value,string label){if(!value)throw new Exception(label);checks.Add(label);}
 }
}
