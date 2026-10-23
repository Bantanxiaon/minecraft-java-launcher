namespace SHLauncher.Infrastructure.Install;
public static class ArchivePathPolicy
{
    public static string NormalizeRelative(string value)
    {
        if (string.IsNullOrWhiteSpace(value) || value.Length > 1024 || value.Any(char.IsControl) || value.Contains(':'))
            throw new InvalidDataException("Archive path is invalid.");
        var normalized = value.Replace('\\','/').TrimEnd('/');
        if (normalized.StartsWith('/') || normalized.Split('/').Any(p=>p is "" or "." or ".."))
            throw new InvalidDataException("Archive path escapes its instance.");
        foreach (var part in normalized.Split('/'))
            if (IsReserved(part) || part.EndsWith('.') || part.EndsWith(' ') || part.IndexOfAny(['<','>','"','|','?','*']) >= 0)
                throw new InvalidDataException("Archive path is not portable.");
        return normalized;
    }
    private static bool IsReserved(string part)
    {
        var stem=part.Split('.')[0].ToUpperInvariant();
        return stem is "CON" or "PRN" or "AUX" or "NUL" ||
            stem.Length==4 && (stem.StartsWith("COM",StringComparison.Ordinal)||stem.StartsWith("LPT",StringComparison.Ordinal)) && stem[3] is >= '1' and <= '9';
    }
    public static string Resolve(string root,string relative)
    {
        var normalized = NormalizeRelative(relative);
        var fullRoot = Path.GetFullPath(root).TrimEnd(Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar;
        var target = Path.GetFullPath(Path.Combine(fullRoot,normalized.Replace('/',Path.DirectorySeparatorChar)));
        if (!target.StartsWith(fullRoot,OperatingSystem.IsWindows()?StringComparison.OrdinalIgnoreCase:StringComparison.Ordinal))
            throw new InvalidDataException("Archive target escapes instance.");
        for (FileSystemInfo? item=new FileInfo(target);item is not null;item=item is DirectoryInfo d?d.Parent:((FileInfo)item).Directory)
            if(IsLinked(item.FullName)) throw new InvalidDataException("Linked archive paths are unsupported.");
        return target;
    }
    private static bool IsLinked(string path)
    {
        try{return (File.GetAttributes(path)&FileAttributes.ReparsePoint)!=0;}
        catch(FileNotFoundException){return false;}
        catch(DirectoryNotFoundException){return false;}
    }

}
