using System.Text.Json;
using SHLauncher.Contracts.Runtime;
namespace SHLauncher.Infrastructure.Runtime;
public sealed class DesktopPreferenceStore(string workspace):IDesktopPreferences
{
    private string SafePath(string name)=>SHLauncher.Infrastructure.Install.ArchivePathPolicy.Resolve(workspace,"settings/"+name);
    private string _path=>SafePath("desktop.json");
    public DesktopPreferences Read()
    {
        if(!File.Exists(_path))return new();if(new FileInfo(_path).Length>4096)throw new InvalidDataException("桌面设置文件超过上限。");
        var result=JsonSerializer.Deserialize<DesktopPreferences>(File.ReadAllText(_path))??throw new InvalidDataException("桌面设置无法读取。");
        if(!Enum.IsDefined(result.CloseAction))throw new InvalidDataException("关闭窗口设置无效。");return result;
    }
    public void Save(DesktopPreferences value)
    {
        if(!Enum.IsDefined(value.CloseAction))throw new ArgumentException("关闭方式无效。");
        Directory.CreateDirectory(Path.GetDirectoryName(_path)!);var pending=SafePath("desktop-"+Guid.NewGuid().ToString("N")+".pending");File.WriteAllText(pending,JsonSerializer.Serialize(value));File.Move(pending,_path,true);
    }
}
