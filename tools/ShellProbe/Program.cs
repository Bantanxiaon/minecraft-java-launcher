using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Layout;
using Avalonia.VisualTree;
using Semi.Avalonia;
using Ursa.Themes.Semi;
using SHLauncher.UI.Shell;
internal static class Program
{
 internal static string Mode="InApp",Output="";
 [STAThread] public static void Main(string[] args){if(args.Length>0)Mode=args[0];if(args.Length>1)Output=args[1];AppBuilder.Configure<ProbeApp>().UsePlatformDetect().StartWithClassicDesktopLifetime(args);}
}
internal sealed class ProbeApp:Avalonia.Application
{
 public override void Initialize(){Styles.Add(new SHLNPresentationTheme());}
 public override void OnFrameworkInitializationCompleted()
 {
  if(ApplicationLifetime is IClassicDesktopStyleApplicationLifetime life)
  {
   var window=new SHLNWindow();life.MainWindow=window;window.SetSceneMaterial(Enum.TryParse<SceneMaterialMode>(Program.Mode,out var mode)?mode:SceneMaterialMode.InApp);window.Title="SHLN · 新外壳 · "+Program.Mode;
   var alternate=false;void Page(){alternate=!alternate;var grid=new Grid{ColumnDefinitions=new ColumnDefinitions("53*,47*")};var content=new StackPanel{Width=280,MaxWidth=280,HorizontalAlignment=HorizontalAlignment.Center,VerticalAlignment=VerticalAlignment.Center,Spacing=12};Grid.SetColumn(content,1);content.Children.Add(new TextBlock{Text=alternate?"外壳原型":"页面切换",FontSize=32});content.Children.Add(new TextBlock{Text="仅验证窗口与覆盖层",Classes={"secondary"}});content.Children.Add(new TextBox{PlaceholderText="表单占位",MaxWidth=280});var example=new Button{Content="操作示例",HorizontalAlignment=HorizontalAlignment.Stretch};example.Click+=(_,_)=>window.ShowToast("这里只验证外壳交互");content.Children.Add(example);grid.Children.Add(content);window.SetPage(grid);}
   void NativeState()
   {
    var directory=Environment.GetEnvironmentVariable("SHLN_SHELL_GATE_EVIDENCE");if(string.IsNullOrWhiteSpace(directory))return;
    Directory.CreateDirectory(directory);var screen=window.Screens.ScreenFromWindow(window);
    var state=new{Utc=DateTime.UtcNow,Mode=Program.Mode,X=window.Position.X,Y=window.Position.Y,Width=window.Bounds.Width,Height=window.Bounds.Height,State=window.WindowState.ToString(),window.RenderScaling,ScreenBounds=screen?.Bounds.ToString(),WorkingArea=screen?.WorkingArea.ToString()};
    File.AppendAllText(Path.Combine(directory,"window-events.jsonl"),System.Text.Json.JsonSerializer.Serialize(state)+Environment.NewLine);
   }
   window.Opened+=(_,_)=>NativeState();window.PositionChanged+=(_,_)=>NativeState();window.SizeChanged+=(_,_)=>NativeState();window.PropertyChanged+=(_,e)=>{if(e.Property==Window.WindowStateProperty)NativeState();};
   Page();Button Action(string label,Action action){var b=new Button{Content=label,Padding=new Thickness(14,8)};b.Click+=(_,_)=>action();return b;}
   window.SetNavigation(new[]{Action("页面",Page),Action("弹窗",async()=>await window.ShowDialogAsync("操作确认",new TextBlock{Text="这是统一 Dialog 的占位内容。",Margin=new Thickness(24)})),Action("表单",async()=>await window.ShowSheetAsync("表单示例",new StackPanel{Margin=new Thickness(24),Spacing=12,Children={new TextBlock{Text="此处承载后续工作流，不执行登录或安装。"},new TextBox{PlaceholderText="输入示例",MinWidth=300}}})),Action("任务",async()=>await window.ShowDrawerAsync("任务中心",new TextBlock{Text="暂无任务 · 外壳原型",Margin=new Thickness(24)})),Action("通知",()=>window.ShowToast("操作示例完成")),Action("减少动态",()=>window.SetReducedMotion(!window.ReducedMotion))});
   if(!string.IsNullOrEmpty(Program.Output))window.Opened+=async(_,_)=>await ProbeAudit.Run(window,Program.Output);
  }
  base.OnFrameworkInitializationCompleted();
 }
}
