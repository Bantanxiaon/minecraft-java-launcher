namespace SHLauncher.Engine.CmlLib;
internal static class PathSafety
{
    internal static string EnsureContained(string root, string path)
    {
        var fullRoot = Path.GetFullPath(root).TrimEnd(Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar;
        var fullPath = Path.GetFullPath(path);
        if (!fullPath.StartsWith(fullRoot, OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal))
            throw new InvalidDataException("Path escapes its allowed root.");
        for (var item = new FileInfo(fullPath) as FileSystemInfo; item is not null; item = item is DirectoryInfo d ? d.Parent : ((FileInfo)item).Directory)
        {
            if (IsLinked(item.FullName)) throw new InvalidDataException("Linked paths are not supported.");
            if (string.Equals(item.FullName.TrimEnd(Path.DirectorySeparatorChar), fullRoot.TrimEnd(Path.DirectorySeparatorChar), OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal)) break;
        }
        return fullPath;
    }
    private static bool IsLinked(string path)
    {
        try{return (File.GetAttributes(path)&FileAttributes.ReparsePoint)!=0;}
        catch(FileNotFoundException){return false;}
        catch(DirectoryNotFoundException){return false;}
    }

}
