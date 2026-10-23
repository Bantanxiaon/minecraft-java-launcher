using System.Security.Cryptography;
using System.Text.Json;
using ICSharpCode.SharpZipLib.Zip;
using ICSharpCode.SharpZipLib.Checksum;
using SHLauncher.Contracts.Install;
namespace SHLauncher.Infrastructure.Install;
public sealed class CurseForgePackInspector : IModpackInspector
{
    public async Task<ModpackPreview> InspectAsync(string archivePath,CancellationToken token)
    {
        if (!Path.IsPathFullyQualified(archivePath) || !Path.GetExtension(archivePath).Equals(".zip",StringComparison.OrdinalIgnoreCase))
            throw new InvalidDataException("请选择一个 ZIP 整合包。");
        await using var input = new FileStream(archivePath,FileMode.Open,FileAccess.Read,FileShare.Read,81920,true);
        if (input.Length > 4L*1024*1024*1024) throw new InvalidDataException("整合包超过当前 4 GiB 检查上限。");
        var hash = Convert.ToHexString(await SHA256.HashDataAsync(input,token).ConfigureAwait(false)); input.Position=0;
        using var zip = new ZipFile(input) { IsStreamOwner=false };
        var entries = zip.Cast<ZipEntry>().ToArray();
        if(entries.Length>100000) throw new InvalidDataException("整合包条目过多。");
        var names = new HashSet<string>(StringComparer.OrdinalIgnoreCase); long total=0;
        foreach(var entry in entries)
        {
            token.ThrowIfCancellationRequested();
            var normalizedName = ArchivePathPolicy.NormalizeRelative(entry.Name);
            if (!names.Add(normalizedName) || entry.IsCrypted || (entry.ExternalFileAttributes&(int)FileAttributes.ReparsePoint)!=0 || ((entry.ExternalFileAttributes>>16)&0xF000)==0xA000 ||
                entry.Size<0 || entry.Size>1024L*1024*1024)
                throw new InvalidDataException("整合包包含重复、加密、链接或过大的条目。");
            total=checked(total+entry.Size); if(total>8L*1024*1024*1024) throw new InvalidDataException("整合包解压预算超过 8 GiB。");
        }
        var manifest=entries.SingleOrDefault(e=>e.Name=="manifest.json") ?? throw new NotSupportedException("当前导入器只接受根目录含 manifest.json 的 CurseForge v1 整合包。");
        if(manifest.Size>2*1024*1024) throw new InvalidDataException("整合包 manifest 过大。");
        using var manifestStream = zip.GetInputStream(manifest);
        using var json=await JsonDocument.ParseAsync(manifestStream,cancellationToken:token).ConfigureAwait(false);
        var data=json.RootElement;
        if(data.GetProperty("manifestType").GetString()!="minecraftModpack" || data.GetProperty("manifestVersion").GetInt32()!=1) throw new NotSupportedException("未知的 CurseForge 格式版本。");
        var minecraft=data.GetProperty("minecraft"); var mc=minecraft.GetProperty("version").GetString();
        var loaders=minecraft.GetProperty("modLoaders").EnumerateArray().ToArray();
        if(loaders.Length!=1) throw new NotSupportedException("当前只支持一个明确指定的 Loader。");
        var loaderId=loaders[0].GetProperty("id").GetString() ?? "";
        var split=loaderId.IndexOf('-'); if(split<=0) throw new InvalidDataException("缺少精确 Loader 版本。");
        var loader=loaderId[..split] switch {"forge"=>LoaderType.Forge,"neoforge"=>LoaderType.NeoForge,"fabric"=>LoaderType.Fabric,"quilt"=>LoaderType.Quilt,_=>throw new NotSupportedException("不支持该 Loader。")};
        var loaderVersion=loaderId[(split+1)..];
        if(string.IsNullOrWhiteSpace(mc)||string.IsNullOrWhiteSpace(loaderVersion)) throw new InvalidDataException("版本信息不足。");
        var prefix=ArchivePathPolicy.NormalizeRelative(data.GetProperty("overrides").GetString() ?? "") + "/";
        var remote=data.GetProperty("files").EnumerateArray().Select(f=>new ProviderFileReference("CurseForge",f.GetProperty("projectID").GetInt64(),f.GetProperty("fileID").GetInt64(),!f.TryGetProperty("required",out var required)||required.GetBoolean())).ToArray();
        if(remote.Any(f=>f.ProjectId<=0||f.FileId<=0)||remote.Length>10000||remote.GroupBy(f=>(f.ProjectId,f.FileId)).Any(g=>g.Count()>1)) throw new InvalidDataException("远程文件身份重复或无效。");
        var local=new List<ArchiveGameFile>();
        foreach(var entry in entries.Where(e=>!e.IsDirectory))
        {
            token.ThrowIfCancellationRequested();
            using var source=zip.GetInputStream(entry); using var sha=IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
            var crc=new Crc32();var buffer=new byte[81920];long done=0;int read;
            while((read=await source.ReadAsync(buffer,token).ConfigureAwait(false))!=0)
            {
                done+=read;if(done>entry.Size)throw new InvalidDataException("ZIP 条目超出声明长度。");
                sha.AppendData(buffer,0,read);crc.Update(new ArraySegment<byte>(buffer,0,read));
            }
            if(done!=entry.Size||crc.Value!=entry.Crc)throw new InvalidDataException("ZIP 条目 CRC 或长度校验失败："+entry.Name);
            var entryHash=Convert.ToHexString(sha.GetHashAndReset());
            if(entry.Name.StartsWith(prefix,StringComparison.Ordinal))
                local.Add(new(entry.Name,ArchivePathPolicy.NormalizeRelative(entry.Name[prefix.Length..]),entry.Size,entryHash));
        }
        var name=data.TryGetProperty("name",out var title)?title.GetString():null;
        if(string.IsNullOrWhiteSpace(name))name=Path.GetFileNameWithoutExtension(archivePath);
        if(name.Length>128)throw new InvalidDataException("整合包名称过长。");
        return new(archivePath,hash,input.Length,ModpackFormat.CurseForge,name,mc,loader,loaderVersion,prefix,remote,local);
    }
}
