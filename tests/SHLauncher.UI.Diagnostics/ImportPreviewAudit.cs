using System.Text.Json;
using System.Security.Cryptography;
using Avalonia;
using Avalonia.Input;
using Avalonia.Controls;
using Avalonia.Media.Imaging;
using Avalonia.VisualTree;
using SHLauncher.Contracts.Downloads;
using SHLauncher.UI.Views;
using SHLauncher.UI.ViewModels;
namespace SHLauncher.UI.Diagnostics;
internal static class ImportPreviewAudit
{
    internal static async Task<bool> RunIfRequested(MainWindow window)
    {
        var folder=Environment.GetEnvironmentVariable("SH_IMPORT_UI_AUDIT");if(string.IsNullOrWhiteSpace(folder))return false;
        Directory.CreateDirectory(folder);var shell=window.Shell;var import=shell.Import;var checks=new List<string>();
        var archive=Environment.GetEnvironmentVariable("SH_IMPORT_UI_ARCHIVE")!;
        var workspace=Environment.GetEnvironmentVariable("SH_DOWNLOAD_UI_WORKSPACE")!;
        void Check(bool value,string message){if(!value)throw new Exception(message);checks.Add(message);}
        async Task Until(Func<bool> condition){using var stop=new CancellationTokenSource(TimeSpan.FromSeconds(40));while(!condition())await Task.Delay(20,stop.Token);}
        async Task Shot(string name){await Task.Delay(350);using var bitmap=new RenderTargetBitmap(new PixelSize((int)window.Bounds.Width,(int)window.Bounds.Height));bitmap.Render(window);bitmap.Save(Path.Combine(folder,name+".png"),PngBitmapEncoderOptions.Default);}
        string Files()=>string.Join("|",Directory.EnumerateFiles(workspace,"*",SearchOption.AllDirectories).Order(StringComparer.Ordinal).Select(p=>Path.GetRelativePath(workspace,p)+":"+new FileInfo(p).Length));
        try
        {
            shell.Page=ShellPage.Library;await Task.Delay(350);
            var origin=window.GetVisualDescendants().OfType<Button>().Single(b=>b.IsEffectivelyVisible&&ReferenceEquals(b.Command,import.ChooseCommand));
            Check(origin.Focus(NavigationMethod.Tab),"visible import entry accepts keyboard focus");
            var before=Files();
            await import.InspectAsync([archive]);
            Check(import.Preview is {MinecraftVersion:"1.20.1",LoaderVersion:"47.3.22"}&&import.Preview.RemoteFiles.Count==271&&import.Preview.LocalFiles.Count==5953,"actual ZIP parsed through production preview service");
            Check(import.ConfirmCommand.CanExecute(null)&&import.IsOpen,"confirmation displayed before enqueue");
            shell.Settings.ThemeIndex=1;await Shot("import-preview-light");
            Check(window.FocusManager?.GetFocusedElement() is Button initial&&ReferenceEquals(initial.Command,import.CancelCommand),"preview defaults to Cancel focus");
            Check(!origin.IsEffectivelyEnabled,"preview disables underlying entry");
            shell.Settings.ThemeIndex=2;await Shot("import-preview-dark");
            window.Width=900;window.Height=640;await Shot("import-preview-small");
            var cancel=window.GetVisualDescendants().OfType<Button>().Single(b=>ReferenceEquals(b.Command,import.CancelCommand));
            cancel.Command!.Execute(null);await Task.Delay(80);
            Check(!import.IsOpen&&Files()==before,"bound confirmation Cancel leaves workspace unchanged");
            Check(ReferenceEquals(window.FocusManager?.GetFocusedElement(),origin),"cancel restores visible import entry focus");
            await import.InspectAsync([archive,archive]);
            Check(import.Preview is null&&!import.ConfirmCommand.CanExecute(null)&&import.Message.Contains("一次只接收"),"multi-file preview rejected without enqueue");
            import.CancelCommand.Execute(null);
            shell.Settings.SourceMode=2;shell.Settings.SaveSourceCommand.Execute(null);await shell.Settings.SaveSourceCommand.DrainAsync();
            Check(File.Exists(Path.Combine(workspace,"settings","downloads.json")),"source setting saved through UI command");
            shell.Settings.ThemeIndex=1;shell.Page=ShellPage.Settings;await Shot("source-setting-light");
            await import.InspectAsync([archive]);
            import.ConfirmCommand.Execute(null);
            await Until(()=>shell.Downloads.Tasks.Any(t=>t.Record.Snapshot.State==DownloadTaskState.Failed));
            var task=shell.Downloads.Tasks.First(t=>t.Record.Snapshot.State==DownloadTaskState.Failed);
            Check(shell.Page==ShellPage.Downloads&&task.Problem.Contains("CurseForge",StringComparison.Ordinal)&&task.Record.Snapshot.Stage=="安装条件未满足"&&!task.IsIndeterminate,"confirmation creates visible truthful missing-access task and stops progress");
            Check(!Directory.Exists(Path.Combine(workspace,"instances")),"no-key stops before instance publication or extraction");
            await Shot("missing-access-task");
            using var original=File.OpenRead(archive);var hash=Convert.ToHexString(await SHA256.HashDataAsync(original));
            Check(hash=="938082714C57683D1EAD5FA46F8CD8855A8261EFB8A1275821AF166A3BE4ACEA","original ZIP unchanged after preview/cancel/refusal");
            File.WriteAllText(Path.Combine(folder,"result.json"),JsonSerializer.Serialize(new{Passed=true,Checks=checks,Note="Production view bindings and services; direct preview invocation is NOT physical drag/drop acceptance. No own CF key deliberately configured in this isolated child."}));
            window.Close();
        }
        catch(Exception error)
        {File.WriteAllText(Path.Combine(folder,"result.json"),JsonSerializer.Serialize(new{Passed=false,Checks=checks,Error=error.ToString()}));window.Close();}
        return true;
    }
}
