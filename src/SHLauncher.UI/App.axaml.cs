using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Markup.Xaml;
using Avalonia.Styling;
using Avalonia.Media;
using Avalonia.Themes.Fluent;
using SHLauncher.UI.Composition;
using SHLauncher.UI.Views;

namespace SHLauncher.UI;

public sealed partial class App : Avalonia.Application
{
    private Shell.ApplicationPresentation? _presentation;
    public override void Initialize()
    {
        AvaloniaXamlLoader.Load(this);

    }
    public override void OnFrameworkInitializationCompleted()
    {
        if (ApplicationLifetime is IClassicDesktopStyleApplicationLifetime desktop)
        {
            desktop.ShutdownMode = ShutdownMode.OnMainWindowClose;
            try
            {
                var shell = DesktopComposition.Create();
                var window = new Shell.SHLNWindow();
                _presentation = new Shell.ApplicationPresentation(window, shell);
                desktop.MainWindow = window;
            }
            catch (Exception error) when (error is System.IO.IOException or UnauthorizedAccessException)
            {
                desktop.MainWindow = new Window { Title = "SH Launcher Next", Width = 500, Height = 220,
                    Content = new TextBlock { Margin = new Thickness(28), TextWrapping = Avalonia.Media.TextWrapping.Wrap,
                        Text = "无法打开启动器数据目录。请检查是否已有启动器正在运行，以及当前文件夹是否允许写入。关闭此窗口后再试。"} };
            }
        }
        base.OnFrameworkInitializationCompleted();
    }
}
