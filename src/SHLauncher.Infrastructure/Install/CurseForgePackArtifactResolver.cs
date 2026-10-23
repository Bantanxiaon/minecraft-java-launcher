using System.Text.Json;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Install;
namespace SHLauncher.Infrastructure.Install;
// Resolves exact manifest identities, without search or name matching. All network goes through the shared metadata source.
public sealed class CurseForgePackArtifactResolver(IJsonMetadataSource metadata, Guid? operationId = null, Action<int,int>? resolvedProgress = null) : IModpackArtifactResolver
{
    public async Task<IReadOnlyList<PlannedArtifact>> ResolveAsync(ModpackPreview preview,CancellationToken token)
    {
        if(preview.Format!=ModpackFormat.CurseForge || preview.RemoteFiles.Any(f=>f.ProviderId!="CurseForge"||f.ProjectId<=0||f.FileId<=0) ||
            preview.RemoteFiles.GroupBy(f=>(f.ProjectId,f.FileId)).Any(g=>g.Count()>1))
            throw new InvalidDataException("CurseForge plan contains invalid or duplicate exact identities.");
        var session = operationId ?? Guid.NewGuid();
        var required=preview.RemoteFiles.Where(f=>f.Required).ToArray();
        var classes = new Dictionary<long,string?>();
        if(required.Length>0)
        {
            using var categories = await Read("https://api.curseforge.com/v1/categories?gameId=432&classesOnly=true");
            classes = categories.RootElement.GetProperty("data").EnumerateArray().ToDictionary(c=>c.GetProperty("id").GetInt64(),c=>c.GetProperty("slug").GetString());
        }
        var result = new List<PlannedArtifact>();
        var resolved=0;resolvedProgress?.Invoke(0,required.Length);
        foreach(var reference in required)
        {
            token.ThrowIfCancellationRequested();
            using var project = await Read("https://api.curseforge.com/v1/mods/"+reference.ProjectId);
            var mod=project.RootElement.GetProperty("data");
            if(mod.GetProperty("id").GetInt64()!=reference.ProjectId||mod.GetProperty("gameId").GetInt32()!=432)throw new InvalidDataException("CurseForge project identity mismatch.");
            var classId=mod.GetProperty("classId").GetInt64();
            if(!classes.TryGetValue(classId,out var slug))throw new InvalidDataException("CurseForge class metadata missing.");
            var (type,directory)=slug switch {
                "mc-mods" => (InstallContentType.Mod,"mods"),
                "texture-packs" => (InstallContentType.ResourcePack,"resourcepacks"),
                "shaders" => (InstallContentType.Shader,"shaderpacks"),
                _ => throw new NotSupportedException("Unsupported CurseForge resource class: "+slug)
            };
            using var file = await Read("https://api.curseforge.com/v1/mods/"+reference.ProjectId+"/files/"+reference.FileId);
            var data=file.RootElement.GetProperty("data");
            if(data.GetProperty("id").GetInt64()!=reference.FileId||data.GetProperty("modId").GetInt64()!=reference.ProjectId)
                throw new InvalidDataException("CurseForge exact file identity mismatch.");
            if(!data.GetProperty("isAvailable").GetBoolean())
                throw new InstallationPreconditionException("ProviderFileUnavailable",$"CurseForge 必需文件不可用（项目 {reference.ProjectId}，文件 {reference.FileId}）；未替换为其他版本。");
            if(mod.TryGetProperty("allowModDistribution",out var distribution)&&distribution.ValueKind==JsonValueKind.False)
                throw new InstallationPreconditionException("ProviderDistributionRestricted",$"作者未允许 CurseForge 第三方分发（项目 {reference.ProjectId}，文件 {reference.FileId}）。请由作者或服务方开放合法访问，未构造下载地址或替换文件。");
            var filename=data.GetProperty("fileName").GetString();
            if(string.IsNullOrWhiteSpace(filename)||filename.Contains('/')||filename.Contains('\\')||filename.Contains(':')||filename is "." or "..")
                throw new InvalidDataException("Unsafe provider filename.");
            var target=ArchivePathPolicy.NormalizeRelative(directory+"/"+filename);
            var versions=data.GetProperty("gameVersions").EnumerateArray().Select(v=>v.GetString()).ToArray();
            if(!versions.Contains(preview.MinecraftVersion))throw new InvalidDataException("Exact modpack file does not declare the selected Minecraft version.");
            if(type==InstallContentType.Mod && !versions.Contains(preview.Loader.ToString(),StringComparer.OrdinalIgnoreCase))
                throw new InvalidDataException("Exact modpack file Loader compatibility is unknown.");
            var hash=data.GetProperty("hashes").EnumerateArray().Where(h=>h.GetProperty("algo").GetInt32()==1).Select(h=>h.GetProperty("value").GetString()).SingleOrDefault();
            if(hash is null||hash.Length!=40||hash.Any(c=>!Uri.IsHexDigit(c)))throw new InvalidDataException("CurseForge exact file lacks SHA1 integrity metadata.");
            var url=data.GetProperty("downloadUrl").GetString();
            if(string.IsNullOrWhiteSpace(url))
                throw new InstallationPreconditionException("ProviderFileDownloadUnavailable",$"CurseForge 未提供必需文件的授权下载地址（项目 {reference.ProjectId}，文件 {reference.FileId}）。请检查自有访问权限及作者的分发限制；未猜测地址或替换文件。");
            if(!Uri.TryCreate(url,UriKind.Absolute,out var uri)||uri.Scheme!="https"||uri.UserInfo.Length!=0)
                throw new InvalidDataException("CurseForge returned an unsafe download address.");
            var size=data.GetProperty("fileLength").GetInt64();if(size<=0)throw new InvalidDataException("Invalid provider file length.");
            result.Add(new("CurseForge",reference.ProjectId+":"+reference.FileId,DestinationScope.InstanceGame,target,type,true,size,hash,null,[new("CurseForge CDN",uri)]));
            resolvedProgress?.Invoke(++resolved,required.Length);
        }
        foreach(var local in preview.LocalFiles)
            result.Add(new("Archive",preview.ArchiveSha256,DestinationScope.InstanceGame,local.GamePath,InstallContentType.Override,true,local.Size,null,local.Sha256,[],local.EntryName));
        return result;
        async Task<JsonDocument> Read(string uri)
        {
            using var stream=await metadata.OpenReadAsync(new Uri(uri),session,token).ConfigureAwait(false);
            return await JsonDocument.ParseAsync(stream,cancellationToken:token).ConfigureAwait(false);
        }
    }
}
