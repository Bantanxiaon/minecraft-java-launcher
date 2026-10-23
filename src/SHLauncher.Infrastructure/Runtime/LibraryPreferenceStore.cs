using System.Text.Json;
namespace SHLauncher.Infrastructure.Runtime;
public sealed record LibraryPreference(string Root,string? SelectedId,bool Managed);
public sealed class LibraryPreferenceStore(string workspace)
{
    private string SafePath(string name) => SHLauncher.Infrastructure.Install.ArchivePathPolicy.Resolve(workspace,"settings/"+name);
    private string _path => SafePath("library.json");
    public LibraryPreference? Read()
    {
        if(!File.Exists(_path))return null;if(new FileInfo(_path).Length>8192)throw new InvalidDataException("资料库设置超过上限。");
        var result=JsonSerializer.Deserialize<LibraryPreference>(File.ReadAllText(_path));
        if(result is null||string.IsNullOrWhiteSpace(result.Root)||!Path.IsPathFullyQualified(result.Root)||result.SelectedId?.Length>256)throw new InvalidDataException("资料库设置无效。");return result;
    }
    public void Save(LibraryPreference value)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(_path)!);var pending=SafePath("library-"+Guid.NewGuid().ToString("N")+".pending");File.WriteAllText(pending,JsonSerializer.Serialize(value));File.Move(pending,_path,true);
    }
}
