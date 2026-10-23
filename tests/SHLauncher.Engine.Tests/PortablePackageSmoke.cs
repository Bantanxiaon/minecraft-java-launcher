using System.Diagnostics;
internal static class PortablePackageSmoke
{
    internal static async Task Run(string folder)
    {
        var exe=Path.Combine(folder,"SHLauncher.UI.exe");
        if(!File.Exists(Path.Combine(folder,"coreclr.dll"))||!File.Exists(Path.Combine(folder,"portable.flag")))throw new Exception("Portable runtime or marker missing");
        var settings=Path.Combine(folder,"data","settings");Directory.CreateDirectory(settings);
        await File.WriteAllTextAsync(Path.Combine(settings,"desktop.json"),"{\"CloseAction\":1,\"TrayNoticeSeen\":true}");
        var start=new ProcessStartInfo(exe){WorkingDirectory=folder,UseShellExecute=false};
        foreach(var name in new[]{"DOTNET_ROOT","SHLAUNCHER_DATA_DIR","SH_CURSEFORGE_API_KEY","SH_CURSEFORGE_CONFIG_FILE","SH_MASTER_ACCOUNT_AUDIT","SH_ONLINE_CATALOG_AUDIT"})start.Environment.Remove(name);
        using var child=Process.Start(start)??throw new Exception("Package did not start");
        try
        {
            using var wait=new CancellationTokenSource(TimeSpan.FromSeconds(45));
            while(!child.HasExited&&child.MainWindowHandle==IntPtr.Zero){await Task.Delay(100,wait.Token);child.Refresh();}
            if(child.HasExited)throw new Exception("Package exited before opening its window");
            await Task.Delay(2500,wait.Token);
            if(!child.CloseMainWindow())throw new Exception("Package window would not accept close");
            await child.WaitForExitAsync(wait.Token);
            if(child.ExitCode!=0)throw new Exception("Package exited unsuccessfully");
            if(!File.Exists(Path.Combine(folder,"data",".owner.lock")))throw new Exception("Package did not use its portable data folder");
            Console.WriteLine("PASS self-contained package opens and exits cleanly without DOTNET_ROOT; data remains beside executable.");
        }
        finally{if(!child.HasExited){child.Kill(true);await child.WaitForExitAsync();}}
    }
}
