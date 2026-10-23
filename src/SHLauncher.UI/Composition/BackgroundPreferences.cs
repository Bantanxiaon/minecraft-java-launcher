using System.Text.Json;
using SHLauncher.UI.Presentation;
namespace SHLauncher.UI.Composition;

internal static class BackgroundPreferences
{
    internal static BackgroundSession Create(string root)
    {
        var path=Path.Combine(root,"settings","background.last.json");
        string? last=null;
        try
        {
            if(Safe(path)&&File.Exists(path)&&new FileInfo(path).Length<=1024)
                last=JsonSerializer.Deserialize<Record>(File.ReadAllText(path))?.Id;
        }
        catch { }
        return new(last,id=>{
            // This optional visual preference must never block startup or accounts.
            try
            {
                if(!Safe(path))return;
                Directory.CreateDirectory(Path.GetDirectoryName(path)!);
                var part=path+"."+Guid.NewGuid().ToString("N")+".part";
                File.WriteAllText(part,JsonSerializer.Serialize(new Record(id)));
                File.Move(part,path,true);
            }
            catch { }
        });
    }
    private static bool Safe(string path)
    {
        for(var current=Path.GetFullPath(path);!string.IsNullOrEmpty(current);current=Path.GetDirectoryName(current))
        {
            try{if((File.GetAttributes(current)&FileAttributes.ReparsePoint)!=0)return false;}
            catch(FileNotFoundException){ }catch(DirectoryNotFoundException){ }
        }
        return true;
    }
    private sealed record Record(string Id);
}
