namespace SHLauncher.Infrastructure.Runtime;
public static class ApplicationDataPaths
{
    public static string Resolve()
    {
        var configured = Environment.GetEnvironmentVariable("SHLAUNCHER_DATA_DIR");
        if (!string.IsNullOrWhiteSpace(configured))
        {
            if (!Path.IsPathFullyQualified(configured)) throw new IOException("Data directory must be absolute.");
            return Path.GetFullPath(configured);
        }
        if (File.Exists(Path.Combine(AppContext.BaseDirectory,"portable.flag"))) return Path.Combine(AppContext.BaseDirectory,"data");
        if (OperatingSystem.IsWindows()) return Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "SHLauncherNext");
        var home = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        if (OperatingSystem.IsMacOS()) return Path.Combine(home, "Library", "Application Support", "SHLauncherNext");
        var xdg = Environment.GetEnvironmentVariable("XDG_DATA_HOME");
        return Path.Combine(!string.IsNullOrWhiteSpace(xdg) && Path.IsPathFullyQualified(xdg) ? xdg : Path.Combine(home, ".local", "share"), "shlauncher-next");
    }
}
