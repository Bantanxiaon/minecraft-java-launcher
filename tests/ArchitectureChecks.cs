using System.Xml.Linq;

internal static class ArchitectureChecks
{
    internal static string Root()
    {
        var directory = new DirectoryInfo(AppContext.BaseDirectory);
        while (directory is not null && !File.Exists(Path.Combine(directory.FullName, "SHLauncherNext.sln")))
            directory = directory.Parent;
        return directory?.FullName ?? throw new InvalidOperationException("Solution root not found.");
    }

    internal static void CheckProject(string name, params string[] allowedReferences)
    {
        var root = Root();
        var file = Path.Combine(root, "src", name, name + ".csproj");
        var project = XDocument.Load(file);
        var actual = project.Descendants("ProjectReference")
            .Select(x => Path.GetFileNameWithoutExtension(x.Attribute("Include")!.Value.Replace('\\', '/')))
            .Order().ToArray();
        if (!actual.SequenceEqual(allowedReferences.Order()))
            throw new InvalidOperationException($"Invalid project dependencies: {name}");
        if (name is not ("SHLauncher.UI" or "SHLauncher.Engine.CmlLib") && project.Descendants("PackageReference").Any(p => !(name == "SHLauncher.Infrastructure" && p.Attribute("Include")?.Value is "SharpZipLib" or "Tomlyn")))
            throw new InvalidOperationException($"Shared project has an unreviewed package: {name}");
        foreach (var target in project.Descendants("TargetFramework"))
            if (target.Value.Contains("-", StringComparison.Ordinal))
                throw new InvalidOperationException($"Platform-specific target: {name}");
        foreach (var source in Directory.EnumerateFiles(Path.GetDirectoryName(file)!, "*.cs", SearchOption.AllDirectories))
        {
            if (source.Split(Path.DirectorySeparatorChar).Any(x => x is "obj" or "bin")) continue;
            var text = File.ReadAllText(source).Replace("System.Windows.Input", "CrossPlatformInput", StringComparison.Ordinal);
            if (name == "SHLauncher.UI" && source.Split(Path.DirectorySeparatorChar).Contains("ViewModels") &&
                new[] { "System.IO", "File.", "Directory.", "ILauncherEngine" }.Any(text.Contains))
                throw new InvalidOperationException("ViewModel crosses the presentation boundary: " + source);
            if (name != "SHLauncher.Downloads" && !(name == "SHLauncher.Engine.CmlLib" && Path.GetFileName(source) == "CmlLibMetadataService.cs") && new[] { "HttpClient", "System.Net", "WebRequest" }.Any(text.Contains))
                throw new InvalidOperationException("Live I/O is not permitted in this shell: " + source);
            if (name == "SHLauncher.UI" && !source.Split(Path.DirectorySeparatorChar).Contains("Composition") && text.Contains("SHLauncher.Infrastructure", StringComparison.Ordinal))
                throw new InvalidOperationException("Concrete engine outside the composition root: " + source);
            if (name is "SHLauncher.Core" or "SHLauncher.Contracts" or "SHLauncher.Application" &&
                new[] { "System.Diagnostics", "File.", "Directory.", "System.Net", "Microsoft.Win32" }.Any(text.Contains))
                throw new InvalidOperationException("Shared layer performs platform IO: " + source);
            if (name == "SHLauncher.UI" && !source.Split(Path.DirectorySeparatorChar).Contains("Composition") &&
                (text.Contains("SHLauncher.Bootstrap", StringComparison.Ordinal) || text.Contains("SHLauncher.Engine.", StringComparison.Ordinal)))
                throw new InvalidOperationException("Concrete services outside composition: " + source);
            string[] forbidden = ["DllImport", "LibraryImport", "Microsoft.Win32", "System.Windows.", "CmlLib.Core", "D:\\", "C:\\", "/Users/", "/home/"];
            foreach (var token in forbidden)
                if (text.Contains(token, StringComparison.Ordinal) && !(name == "SHLauncher.Engine.CmlLib" && token == "CmlLib.Core") && !(name == "SHLauncher.Platform" && (Path.GetFileName(source) == "WindowsCredentialProtector.cs" || (Path.GetFileName(source) == "SystemMemoryService.cs" && text.Contains("GlobalMemoryStatusEx(ref Status status)") && text.Contains("if(!OperatingSystem.IsWindows())throw new PlatformNotSupportedException"))) && token == "DllImport"))
                    throw new InvalidOperationException($"Forbidden dependency or path in {source}: {token}");
        }
    }
}
