using SHLauncher.Bootstrap;
using SHLauncher.UI.ViewModels;
namespace SHLauncher.UI.Composition;
internal static class DesktopComposition
{
    public static ShellViewModel Create()
    {
        var scenario = "Success";
#if DEBUG
        scenario = Environment.GetEnvironmentVariable("SH_MOCK_SCENARIO") ?? "Success";
#endif
        var workspace = LauncherComposition.DefaultDataDirectory();
        #if DEBUG
        workspace = Environment.GetEnvironmentVariable("SH_DOWNLOAD_UI_WORKSPACE") ?? workspace;
#endif
        var composition = new LauncherComposition(workspace, scenario);
        var shell = new ShellViewModel(new LaunchViewModel(composition.Coordinator, composition.LauncherLog), composition, BackgroundPreferences.Create(workspace));
        shell.DesktopPreferences=new SHLauncher.Infrastructure.Runtime.DesktopPreferenceStore(workspace);shell.ProcessMemory=new SHLauncher.Platform.ProcessMemory();
        shell.Mods=new(shell.Launch,composition.Mods,new(composition.OnlineModProviders,composition.OnlineMods));shell.Library.ModsCommand=shell.ModsCommand;
        shell.Memory=composition.Memory;shell.MemoryJava=composition.ProbeMemoryJavaAsync;
        shell.ProfileTextures=composition.ProfileTextures;
        shell.InitializeLibraryAsync = () => composition.InitializePresentationLibraryAsync(CancellationToken.None);
        return shell;
    }
}
