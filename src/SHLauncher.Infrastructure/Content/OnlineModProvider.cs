using System.Text.Json;
using SHLauncher.Contracts.Content;
using SHLauncher.Contracts.Install;
namespace SHLauncher.Infrastructure.Content;
/// <summary>Provider metadata only. All requests reuse the production metadata/download pipeline.</summary>
public sealed class OnlineModProvider(string id,IJsonMetadataSource metadata,Func<bool> hasAccess):IOnlineModProvider
{
    public string Id{get;}=id is "Modrinth" or "CurseForge"?id:throw new ArgumentException("Unknown provider",nameof(id));
    private bool Modrinth=>Id=="Modrinth";
    private static string Escape(string text)=>Uri.EscapeDataString(text);
    private static string JsonArray(string text)=>Escape(JsonSerializer.Serialize(new[]{text}));
    private static string Text(JsonElement value,string name)=>value.TryGetProperty(name,out var property)&&property.ValueKind==JsonValueKind.String?property.GetString()!:"";
    private static string[] Strings(JsonElement value,string name)=>value.GetProperty(name).EnumerateArray().Select(v=>v.GetString()??"").ToArray();
    private async Task<JsonDocument> Read(string path,CancellationToken token)
    {
        if(!Modrinth&&!hasAccess())throw new IOException("请先配置 SHLN 自有的 CurseForge 访问权限。");
        using var stream=await metadata.OpenReadAsync(new Uri((Modrinth?"https://api.modrinth.com/v2/":"https://api.curseforge.com/v1/")+path),Guid.NewGuid(),token).ConfigureAwait(false);
        return await JsonDocument.ParseAsync(stream,new JsonDocumentOptions{MaxDepth=48},token).ConfigureAwait(false);
    }
    private static void Context(InstanceCompatibilityContext context)
    {
        if(string.IsNullOrWhiteSpace(context.MinecraftVersion)||context.Loader.ToLowerInvariant() is not ("fabric" or "forge" or "neoforge" or "quilt"))throw new IOException("请选择已安装 Mod Loader 的实例。");
    }
    private static int Loader(string loader)=>loader.ToLowerInvariant() switch{"forge"=>1,"fabric"=>4,"quilt"=>5,"neoforge"=>6,_=>throw new IOException("不支持此 Loader。")};
    public async Task<IReadOnlyList<ContentSearchResult>> SearchAsync(ContentSearchQuery query,CancellationToken token)
    {
        if(query.Type!=ContentType.Mod)throw new NotSupportedException("此入口仅用于 Mod。");
        var context=new InstanceCompatibilityContext(query.MinecraftVersion??"",query.Loader??"",null);Context(context);
        var facets=Escape(JsonSerializer.Serialize(new[]{new[]{"project_type:mod"},new[]{"versions:"+context.MinecraftVersion},new[]{"categories:"+context.Loader.ToLowerInvariant()}}));
        using var json=await Read(Modrinth?$"search?query={Escape(query.Text)}&facets={facets}&limit=40":$"mods/search?gameId=432&classId=6&sortField=2&sortOrder=desc&searchFilter={Escape(query.Text)}&gameVersion={Escape(context.MinecraftVersion)}&modLoaderType={Loader(context.Loader)}&pageSize=40",token);
        return json.RootElement.GetProperty(Modrinth?"hits":"data").EnumerateArray().Take(40).Select(v=>new ContentSearchResult(Id,Modrinth?Text(v,"project_id"):v.GetProperty("id").GetInt64().ToString(System.Globalization.CultureInfo.InvariantCulture),Text(v,Modrinth?"title":"name"),ContentType.Mod)).ToArray();
    }
    public async Task<IReadOnlyList<OnlineModFile>> VersionsAsync(string projectId,InstanceCompatibilityContext context,CancellationToken token)
    {
        Context(context);await CheckProject(projectId,token);
        using var json=await Read(Modrinth?$"project/{Escape(projectId)}/version?loaders={JsonArray(context.Loader.ToLowerInvariant())}&game_versions={JsonArray(context.MinecraftVersion)}&include_changelog=false":$"mods/{Escape(projectId)}/files?gameVersion={Escape(context.MinecraftVersion)}&modLoaderType={Loader(context.Loader)}&pageSize=50",token);
        var array=Modrinth?json.RootElement:json.RootElement.GetProperty("data");var files=new List<OnlineModFile>();
        foreach(var element in array.EnumerateArray().Take(500))
        {
            token.ThrowIfCancellationRequested();
            // An unavailable version is excluded; malformed or contradictory metadata is not silently accepted.
            if(!Modrinth&&(!element.GetProperty("isAvailable").GetBoolean()||string.IsNullOrWhiteSpace(Text(element,"downloadUrl"))))continue;
            var file=Parse(element,projectId);
            if(file.Version.MinecraftVersions.Contains(context.MinecraftVersion)&&file.Version.Loaders.Contains(context.Loader,StringComparer.OrdinalIgnoreCase))files.Add(file);
        }
        return files.OrderByDescending(v=>v.Version.PublishedAt).ToArray();
    }
    public async Task<OnlineModFile> ExactVersionAsync(string projectId,string versionId,CancellationToken token)
    {
        if(Modrinth&&string.IsNullOrEmpty(projectId))
        {
            using var exactMetadata=await Read("version/"+Escape(versionId),token);
            projectId=Text(exactMetadata.RootElement,"project_id");
        }
        await CheckProject(projectId,token);
        using var json=await Read(Modrinth?"version/"+Escape(versionId):$"mods/{Escape(projectId)}/files/{Escape(versionId)}",token);
        var result=Parse(Modrinth?json.RootElement:json.RootElement.GetProperty("data"),projectId);
        if(result.Version.VersionId!=versionId)throw new InvalidDataException("内容版本身份不匹配。");return result;
    }
    private async Task CheckProject(string project,CancellationToken token)
    {
        if(string.IsNullOrWhiteSpace(project)||project.Length>128)throw new InvalidDataException("项目 ID 无效。");
        using var json=await Read((Modrinth?"project/":"mods/")+Escape(project),token);var item=Modrinth?json.RootElement:json.RootElement.GetProperty("data");
        if(Modrinth){if(Text(item,"id")!=project||Text(item,"project_type")!="mod")throw new InvalidDataException("目标不是对应的 Mod 项目。");}
        else
        {
            if(item.GetProperty("id").GetInt64().ToString(System.Globalization.CultureInfo.InvariantCulture)!=project||item.GetProperty("gameId").GetInt32()!=432||item.GetProperty("classId").GetInt32()!=6)throw new InvalidDataException("CurseForge 项目身份或类型不匹配。");
            if(item.TryGetProperty("allowModDistribution",out var allowed)&&allowed.ValueKind==JsonValueKind.False)throw new IOException("作者未允许第三方下载此 Mod；未构造替代下载地址。");
        }
    }
    private OnlineModFile Parse(JsonElement value,string project)
    {
        string versionId,version,name,sha,url;long size;string[] games,loaders;DateTimeOffset date;ReleaseChannel channel;var dependencies=new List<DependencyEdge>();
        if(Modrinth)
        {
            if(Text(value,"project_id")!=project)throw new InvalidDataException("Modrinth 项目身份不匹配。");
            versionId=Text(value,"id");version=Text(value,"version_number");games=Strings(value,"game_versions");loaders=Strings(value,"loaders");date=value.GetProperty("date_published").GetDateTimeOffset();
            channel=Text(value,"version_type") switch{"release"=>ReleaseChannel.Release,"beta"=>ReleaseChannel.Beta,"alpha"=>ReleaseChannel.Alpha,_=>ReleaseChannel.Unknown};
            var choices=value.GetProperty("files").EnumerateArray().Where(f=>Text(f,"filename").EndsWith(".jar",StringComparison.OrdinalIgnoreCase)).ToArray();
            var primary=choices.Where(f=>f.TryGetProperty("primary",out var flag)&&flag.ValueKind==JsonValueKind.True).ToArray();
            var file=primary.Length==1?primary[0]:choices.Length==1?choices[0]:throw new InvalidDataException("此版本没有唯一可确定的主 JAR。");
            name=Text(file,"filename");size=file.GetProperty("size").GetInt64();sha=Text(file.GetProperty("hashes"),"sha1");url=Text(file,"url");
            foreach(var edge in value.GetProperty("dependencies").EnumerateArray())
            {
                var kind=Text(edge,"dependency_type") switch{"required"=>DependencyKind.Required,"optional"=>DependencyKind.Optional,"incompatible"=>DependencyKind.Incompatible,"embedded"=>DependencyKind.Embedded,_=>DependencyKind.Unknown};
                var target=Text(edge,"project_id");var exact=Text(edge,"version_id");
                dependencies.Add(new(new(Id,target),kind,new(string.IsNullOrEmpty(exact)?null:exact,null,null)));
            }
        }
        else
        {
            if(value.GetProperty("modId").GetInt64().ToString(System.Globalization.CultureInfo.InvariantCulture)!=project||!value.GetProperty("isAvailable").GetBoolean())throw new InvalidDataException("CurseForge 文件不可用或身份不匹配。");
            versionId=value.GetProperty("id").GetInt64().ToString(System.Globalization.CultureInfo.InvariantCulture);version=Text(value,"displayName");name=Text(value,"fileName");size=value.GetProperty("fileLength").GetInt64();url=Text(value,"downloadUrl");
            sha=value.GetProperty("hashes").EnumerateArray().Where(h=>h.GetProperty("algo").GetInt32()==1).Select(h=>Text(h,"value")).SingleOrDefault()??"";
            var all=Strings(value,"gameVersions");loaders=all.Where(v=>v.ToLowerInvariant() is "fabric" or "forge" or "neoforge" or "quilt").ToArray();games=all.Except(loaders).ToArray();date=value.GetProperty("fileDate").GetDateTimeOffset();
            channel=value.GetProperty("releaseType").GetInt32() switch{1=>ReleaseChannel.Release,2=>ReleaseChannel.Beta,3=>ReleaseChannel.Alpha,_=>ReleaseChannel.Unknown};
            foreach(var edge in value.GetProperty("dependencies").EnumerateArray())dependencies.Add(new(new(Id,edge.GetProperty("modId").GetInt64().ToString(System.Globalization.CultureInfo.InvariantCulture)),edge.GetProperty("relationType").GetInt32() switch{1=>DependencyKind.Embedded,2=>DependencyKind.Optional,3=>DependencyKind.Required,4=>DependencyKind.Embedded,5=>DependencyKind.Incompatible,_=>DependencyKind.Unknown},new(null,null,null)));
        }
        if(string.IsNullOrWhiteSpace(versionId)||string.IsNullOrWhiteSpace(name)||name!=Path.GetFileName(name)||name.IndexOfAny(Path.GetInvalidFileNameChars())>=0||name.Contains(':')||!name.EndsWith(".jar",StringComparison.OrdinalIgnoreCase)||size<=0||size>512L*1024*1024||sha.Length!=40||sha.Any(c=>!Uri.IsHexDigit(c)))throw new InvalidDataException("Mod 文件名、大小或完整性信息无效。");
        if(!Uri.TryCreate(url,UriKind.Absolute,out var uri)||uri.Scheme!="https"||uri.UserInfo.Length!=0||uri.Port!=443)throw new IOException("服务未提供合法下载地址，未猜测或替换。");
        return new(new(new(Id,project),versionId,version,channel,games,loaders,dependencies,date),name,size,sha,uri);
    }
}
