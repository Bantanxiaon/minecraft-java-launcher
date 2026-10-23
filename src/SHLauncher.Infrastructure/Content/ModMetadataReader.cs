using System.IO.Compression;
using System.Text;
using System.Text.Json;
using Tomlyn;
using Tomlyn.Model;
using SHLauncher.Contracts.Content;
namespace SHLauncher.Infrastructure.Content;
public static class ModMetadataReader
{
    public static LocalMod Read(string path)
    {
        var info=new FileInfo(path);var filename=info.Name;var enabled=filename.EndsWith(".jar",StringComparison.OrdinalIgnoreCase);
        var fallback=new LocalMod(filename,path,enabled,"",filename,"未知","未知",[],"",[],null,null,info.Length,info.LastWriteTimeUtc);
        try
        {
            if(info.Length>512L*1024*1024)throw new InvalidDataException("JAR 超过扫描上限。");
            using var stream=new FileStream(path,FileMode.Open,FileAccess.Read,FileShare.ReadWrite|FileShare.Delete);using var zip=new ZipArchive(stream,ZipArchiveMode.Read);
            if(zip.Entries.Count>50000)throw new InvalidDataException("JAR 条目过多。");
            var manifest=ReadText(zip,"META-INF/MANIFEST.MF");
            var fields=new Dictionary<string,string>(StringComparer.OrdinalIgnoreCase);string? previous=null;
            foreach(var line in manifest.Replace("\r","").Split('\n')){if(line.StartsWith(' ')&&previous is not null){fields[previous]+=line[1..];continue;}var at=line.IndexOf(':');if(at>0){previous=line[..at];fields[previous]=line[(at+1)..].Trim();}}
            var manifestVersion=fields.GetValueOrDefault("Implementation-Version","未知");
            var fabric=ReadText(zip,"fabric.mod.json");
            if(fabric.Length>0)
            {
                using var json=JsonDocument.Parse(fabric,new JsonDocumentOptions{MaxDepth=32});var root=json.RootElement;
                var deps=new List<ModDependency>();foreach(var key in new[]{"depends","recommends","suggests","breaks"})if(root.TryGetProperty(key,out var d)&&d.ValueKind==JsonValueKind.Object)foreach(var dep in d.EnumerateObject())deps.Add(new(dep.Name,dep.Value.ToString(),key=="depends"));
                var icon=root.TryGetProperty("icon",out var i)?i.ValueKind==JsonValueKind.String?i.GetString():i.ValueKind==JsonValueKind.Object?i.EnumerateObject().FirstOrDefault().Value.ToString():null:null;
                return fallback with{Id=Text(root,"id"),Name=Text(root,"name",Text(root,"id",filename)),Version=Text(root,"version",manifestVersion),Loader="Fabric",Authors=Authors(root,"authors"),Description=Text(root,"description"),Dependencies=deps,Icon=ReadIcon(zip,icon)};
            }
            var quilt=ReadText(zip,"quilt.mod.json");
            if(quilt.Length>0)
            {
                using var json=JsonDocument.Parse(quilt,new JsonDocumentOptions{MaxDepth=32});var loader=json.RootElement.GetProperty("quilt_loader");var meta=loader.TryGetProperty("metadata",out var m)?m:default;
                var deps=new List<ModDependency>();if(loader.TryGetProperty("depends",out var d)&&d.ValueKind==JsonValueKind.Array)foreach(var item in d.EnumerateArray())deps.Add(item.ValueKind==JsonValueKind.String?new(item.GetString()!,"*",true):new(Text(item,"id"),item.TryGetProperty("versions",out var versions)?versions.ToString():"*",!item.TryGetProperty("optional",out var optional)||optional.ValueKind!=JsonValueKind.True));
                var authors=meta.ValueKind==JsonValueKind.Object&&meta.TryGetProperty("contributors",out var contributors)&&contributors.ValueKind==JsonValueKind.Object?contributors.EnumerateObject().Select(c=>c.Name).ToArray():[];
                return fallback with{Id=Text(loader,"id"),Name=Text(meta,"name",Text(loader,"id",filename)),Version=Text(loader,"version",manifestVersion),Loader="Quilt",Authors=authors,Description=Text(meta,"description"),Dependencies=deps,Icon=ReadIcon(zip,Text(meta,"icon"))};
            }
            var toml=ReadText(zip,"META-INF/neoforge.mods.toml");var kind="NeoForge";if(toml.Length==0){toml=ReadText(zip,"META-INF/mods.toml");kind="Forge";}
            if(toml.Length>0)
            {
                var data=TomlSerializer.Deserialize<TomlTable>(toml,new TomlSerializerOptions{MaxDepth=32})!;
                var mods=data.TryGetValue("mods",out var list)&&list is System.Collections.IEnumerable entries?entries.Cast<object>().OfType<TomlTable>().ToArray():[];
                if(mods.Length==0)throw new InvalidDataException("未找到 Mod 元数据。");
                string T(TomlTable table,string key,string value="")=>table.TryGetValue(key,out var v)?v?.ToString()??value:value;
                var ids=mods.Select(m=>T(m,"modId")).ToArray();var deps=new List<ModDependency>();
                if(data.TryGetValue("dependencies",out var ds)&&ds is TomlTable dependencies)foreach(var id in ids)if(dependencies.TryGetValue(id,out var dl)&&dl is System.Collections.IEnumerable items)foreach(var dep in items.Cast<object>().OfType<TomlTable>())deps.Add(new(T(dep,"modId"),T(dep,"versionRange","*"),T(dep,"mandatory")=="True"||T(dep,"type")=="required"));
                var first=mods[0];return fallback with{Id=string.Join(",",ids),Name=string.Join(" / ",mods.Select(m=>T(m,"displayName",T(m,"modId",filename)))),Version=T(first,"version",manifestVersion).Replace("${file.jarVersion}",manifestVersion),Loader=kind,Authors=[T(first,"authors")],Description=T(first,"description"),Dependencies=deps,Icon=ReadIcon(zip,T(first,"logoFile"))};
            }
            var legacy=ReadText(zip,"mcmod.info");
            if(legacy.Length>0)
            {
                using var json=JsonDocument.Parse(legacy,new JsonDocumentOptions{MaxDepth=32});var root=json.RootElement;var list=root.ValueKind==JsonValueKind.Array?root:root.GetProperty("modList");var first=list.EnumerateArray().First();
                return fallback with{Id=Text(first,"modid"),Name=Text(first,"name",filename),Version=Text(first,"version",manifestVersion),Loader="Forge",Authors=Authors(first,"authorList"),Description=Text(first,"description"),Icon=ReadIcon(zip,Text(first,"logoFile"))};
            }
            return fallback with{Name=fields.GetValueOrDefault("Implementation-Title",filename),Version=manifestVersion,Problem="未识别元数据，按文件名显示；兼容性未知。"};
        }
        catch(Exception e) when(e is not OutOfMemoryException and not StackOverflowException)
        {return fallback with{Problem="无法读取 Mod 元数据："+e.GetType().Name};}
    }
    private static string Text(JsonElement element,string key,string fallback="")=>element.ValueKind==JsonValueKind.Object&&element.TryGetProperty(key,out var value)&&value.ValueKind==JsonValueKind.String?value.GetString()??fallback:fallback;
    private static string[] Authors(JsonElement root,string key)=>root.TryGetProperty(key,out var list)&&list.ValueKind==JsonValueKind.Array?list.EnumerateArray().Select(v=>v.ValueKind==JsonValueKind.String?v.GetString()!:Text(v,"name")).ToArray():[];
    private static string ReadText(ZipArchive zip,string name)=>zip.GetEntry(name) is {} entry?Encoding.UTF8.GetString(ReadEntry(entry,1024*1024)):"";
    private static byte[] ReadEntry(ZipArchiveEntry entry,int limit)
    {
        if(entry.Length>limit)throw new InvalidDataException("元数据超过上限。");using var input=entry.Open();using var output=new MemoryStream();var buffer=new byte[8192];int read;
        while((read=input.Read(buffer))>0){if(output.Length+read>limit)throw new InvalidDataException();output.Write(buffer,0,read);}return output.ToArray();
    }
    private static byte[]? ReadIcon(ZipArchive zip,string? name)
    {
        if(string.IsNullOrWhiteSpace(name)||name.Contains("..",StringComparison.Ordinal)||name.StartsWith('/')||name.Contains('\\'))return null;
        try{return zip.GetEntry(name) is {} entry?ReadEntry(entry,128*1024):null;}catch(InvalidDataException){return null;}
    }
}
