using Avalonia;
using Avalonia.Controls;
using Avalonia.Layout;
using Avalonia.Threading;
using SHLauncher.Contracts.Runtime;
using SHLauncher.Application.Runtime;
using SHLauncher.UI.ViewModels;
namespace SHLauncher.UI.Shell;
public sealed class MemoryPanel:UserControl,IAsyncDisposable
{
    private readonly ShellViewModel _model;
    private readonly ComboBox _scope=new(){ItemsSource=new[]{"全局设置","当前实例单独配置"},SelectedIndex=0};
    private readonly CheckBox _inherit=new(){Content="使用全局设置",IsChecked=true,IsVisible=false};
    private readonly ComboBox _mode=new(){ItemsSource=new[]{"自动配置（推荐）","自定义"},SelectedIndex=0};
    private readonly NumericUpDown _custom=new(){Minimum=512,Maximum=1048576,Increment=256,Value=4096,FormatString="0 'MiB'"};
    private readonly TextBlock _stats=new(){TextWrapping=Avalonia.Media.TextWrapping.Wrap};
    private readonly TextBlock _detail=new(){TextWrapping=Avalonia.Media.TextWrapping.Wrap,Opacity=.7,FontSize=12};
    private readonly TextBlock _message=new(){TextWrapping=Avalonia.Media.TextWrapping.Wrap};
    private readonly ProgressBar _used=new(){Minimum=0,Maximum=100,Height=6};
    private readonly DispatcherTimer _timer=new(){Interval=TimeSpan.FromSeconds(5)};
    private readonly CancellationTokenSource _lifetime=new();
    private bool _loading;private Task _pending=Task.CompletedTask;private string? _javaKey;private JavaRuntime? _java;
    public MemoryPanel(ShellViewModel model)
    {
        _model=model;
        var save=new Button{Content="保存内存设置",HorizontalAlignment=HorizontalAlignment.Left};
        Content=new StackPanel{Spacing=12,Children={new TextBlock{Text="游戏内存",FontSize=18},_scope,_inherit,_mode,_custom,save,_stats,_used,_detail,_message}};
        _scope.SelectionChanged+=(_,_)=>ReadPreference();_inherit.IsCheckedChanged+=(_,_)=>UpdateControls();_mode.SelectionChanged+=(_,_)=>UpdateControls();
        save.Click+=(_,_)=>Save();
        _timer.Tick+=(_,_)=>Refresh();Loaded+=(_,_)=>{ReadPreference();Refresh();_timer.Start();};Unloaded+=(_,_)=>_timer.Stop();
    }
    private void ReadPreference()
    {
        try
        {
            var config=_model.Memory!.Configuration;var selected=_model.Launch.Selected;
            var own=_scope.SelectedIndex==1&&selected is not null;
            _inherit.IsVisible=own;var pref=config.Global;
            if(own){_inherit.IsChecked=!config.Instances.TryGetValue(GameMemoryService.InstanceKey(selected!),out var local);pref=local??config.Global;}
            _mode.SelectedIndex=pref.Automatic?0:1;_custom.Value=pref.CustomMb;UpdateControls();
        }
        catch{_message.Text="内存设置无法读取，原文件已保留。";}
    }
    private void UpdateControls(){var editable=_scope.SelectedIndex==0||_inherit.IsChecked!=true;_mode.IsEnabled=editable;_custom.IsEnabled=editable&&_mode.SelectedIndex==1;}
    private void Save()
    {
        try
        {
            var service=_model.Memory!;var config=service.Configuration;var pref=new MemoryPreference(_mode.SelectedIndex==0,(int)(_custom.Value??4096));
            if(_scope.SelectedIndex==0)config=config with{Global=pref};
            else if(_model.Launch.Selected is {} selected){var key=GameMemoryService.InstanceKey(selected);if(_inherit.IsChecked==true)config.Instances.Remove(key);else config.Instances[key]=pref;}
            else{_message.Text="请先选择实例。";return;}
            service.Save(config);_message.Text="内存设置已保存。";Refresh();
        }
        catch{_message.Text="内存设置未能保存，请检查数值和存储位置。";}
    }
    private void Refresh(){if(!_loading&&!_lifetime.IsCancellationRequested)_pending=RefreshAsync();}
    private async Task RefreshAsync()
    {
        _loading=true;
        try
        {
            var selected=_model.Launch.Selected;if(selected is null){_stats.Text="选择实例后显示内存预算。";return;}
            var key=selected.Id+"|"+selected.JavaMajor+"|"+_model.Settings.JavaPath;
            if(key!=_javaKey){_java=_model.MemoryJava is {} probe?await probe(selected,_lifetime.Token):null;_javaKey=key;}
            var result=await _model.Memory!.InspectAsync(selected,_java,selected.Location?.GameDirectory??"",_lifetime.Token);
            if(_lifetime.IsCancellationRequested)return;
            _stats.Text=$"游戏 {result.ResolvedMb/1024d:0.0} GB · 系统已用 {(result.System.TotalMb-result.System.AvailableMb)/1024d:0.0} GB · 总内存 {result.System.TotalMb/1024d:0.0} GB";
            _used.Value=100d*(result.System.TotalMb-result.System.AvailableMb)/result.System.TotalMb;
            _detail.Text=$"设置预算 {result.RequestedMb/1024d:0.0} GB · 当前上限 {result.LimitMb/1024d:0.0} GB · {result.ModCount} 个启用 Mod\n"+(_java is null?"未找到匹配的 Java；启动前将阻止不兼容环境。":$"Java {_java.Version} · {_java.Architecture}")+"\n"+(result.Problem??result.Explanation);
        }
        catch(OperationCanceledException) { }
        catch{_stats.Text="暂时无法读取内存预算；启动前会重新检查。";}
        finally{_loading=false;}
    }
    public async ValueTask DisposeAsync(){_timer.Stop();_lifetime.Cancel();await _pending;_lifetime.Dispose();}
}
