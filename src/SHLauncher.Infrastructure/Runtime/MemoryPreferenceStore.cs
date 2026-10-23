using System.Text.Json;
using SHLauncher.Contracts.Runtime;
namespace SHLauncher.Infrastructure.Runtime;
public sealed class MemoryPreferenceStore(string workspace):IMemoryPreferenceStore
{
    private readonly string _path=Path.Combine(workspace,"settings","game-memory.json");
    private readonly object _sync=new();
    public MemoryConfiguration Read()
    {
        lock(_sync){if(!File.Exists(_path))return new(new(),new());if(new FileInfo(_path).Length>1024*1024)throw new InvalidDataException("内存设置文件过大，已保留原文件。");
        var value=JsonSerializer.Deserialize<MemoryConfiguration>(File.ReadAllText(_path))??throw new InvalidDataException("内存设置无效。");Validate(value);return value;}
    }
    public void Save(MemoryConfiguration value)
    {
        Validate(value);lock(_sync){Directory.CreateDirectory(Path.GetDirectoryName(_path)!);var pending=_path+".pending";File.WriteAllText(pending,JsonSerializer.Serialize(value,new JsonSerializerOptions{WriteIndented=true}));File.Move(pending,_path,true);}
    }
    private static void Validate(MemoryConfiguration value)
    {
        if(value.SchemaVersion!=1||value.Global is null||value.Instances is null||value.Instances.Count>1000)throw new InvalidDataException("内存设置结构无效。");
        foreach(var pref in value.Instances.Values.Append(value.Global))if(pref is null||pref.CustomMb is <512 or >1048576)throw new InvalidDataException("自定义内存数值无效。");
    }
}
