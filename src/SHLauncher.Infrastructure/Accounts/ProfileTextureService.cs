using System.Buffers.Binary;
using System.Text.Json;
using SHLauncher.Contracts.Accounts;
namespace SHLauncher.Infrastructure.Accounts;

/// <summary>Public profile lookups use the existing bounded, public-HTTPS account transport.
/// No credentials, alternate HTTP client, or launch dependency are introduced.</summary>
public sealed class ProfileTextureService(IAuthenticationTransport transport,string workspace):IProfileTextureService
{
    private static readonly TextureCapabilities None=new(false,false,false,false,false);
    private readonly SemaphoreSlim _gate=new(1,1);
    private readonly string _cache=Path.Combine(workspace,"presentation","textures");
    public async Task<ProfileTextures> ReadAsync(MinecraftAccount account,AuthenticationAuthority? authority,bool force,CancellationToken token)
    {
        await _gate.WaitAsync(token);
        try{return await ReadCore(account,authority,force,token);}
        finally{_gate.Release();}
    }
    private async Task<ProfileTextures> ReadCore(MinecraftAccount account,AuthenticationAuthority? authority,bool force,CancellationToken token)
    {
        if(account.AccountType!=MinecraftAccountType.Yggdrasil||authority is null)return new(null,null,None,false,"使用 SHLN 默认皮肤");
        var caps=new TextureCapabilities(true,false,true,false,false);
        var key=account.AccountId.ToString("N");
        var snapshotPath=Path.Combine(_cache,key+".json");
        var snapshot=await CachedSnapshot(snapshotPath,token);
        var cachedSkin=snapshot?.Skin;var cachedCape=snapshot?.Cape;
        if(!force&&snapshot is not null&&snapshot.Updated>DateTimeOffset.UtcNow.AddHours(-6))return new(cachedSkin,cachedCape,caps,true,cachedSkin is null?"此角色尚未设置皮肤":"已缓存的角色纹理");
        try
        {
            var response=await transport.SendAsync(new Uri(authority.ApiRoot,"sessionserver/session/minecraft/profile/"+account.ProfileId.ToString("N")),"GET",null,token);
            if(response.StatusCode!=200)throw new InvalidDataException();
            using var profile=JsonDocument.Parse(response.Body,new JsonDocumentOptions{MaxDepth=16});
            var root=profile.RootElement;
            if(!Guid.TryParse(root.GetProperty("id").GetString(),out var id)||id!=account.ProfileId)throw new InvalidDataException();
            var properties=root.GetProperty("properties");if(properties.GetArrayLength()>32)throw new InvalidDataException();
            var encoded=properties.EnumerateArray().FirstOrDefault(v=>v.GetProperty("name").GetString()=="textures");
            if(encoded.ValueKind==JsonValueKind.Undefined){await SaveSnapshot(snapshotPath,new(null,null,DateTimeOffset.UtcNow),token);return new(null,null,caps,false,"此角色尚未设置纹理");}
            using var textures=JsonDocument.Parse(Convert.FromBase64String(encoded.GetProperty("value").GetString()!),new JsonDocumentOptions{MaxDepth=16});
            var data=textures.RootElement;
            if(data.TryGetProperty("profileId",out var profileId)&&(!Guid.TryParse(profileId.GetString(),out var textureId)||textureId!=account.ProfileId))throw new InvalidDataException();
            var entries=data.GetProperty("textures");
            var skin=await Fetch(entries,"SKIN",true,token);var cape=await Fetch(entries,"CAPE",false,token);
            await SaveSnapshot(snapshotPath,new(skin,cape,DateTimeOffset.UtcNow),token);
            return new(skin,cape,caps,false,"来自认证服务的角色纹理");
        }
        catch(OperationCanceledException) when(token.IsCancellationRequested){throw;}
        catch(Exception e) when(e is AccountException or InvalidDataException or IOException or JsonException or FormatException or InvalidOperationException or KeyNotFoundException or ArgumentException)
        {return new(cachedSkin,cachedCape,caps,cachedSkin is not null,cachedSkin is null?"纹理暂不可用，使用 SHLN 默认皮肤":"纹理刷新失败，使用有效缓存");}
    }
    private async Task<byte[]?> Fetch(JsonElement textures,string kind,bool skin,CancellationToken token)
    {
        if(!textures.TryGetProperty(kind,out var entry))return null;
        var url=entry.GetProperty("url").GetString();if(url is null||url.Length>2048||!Uri.TryCreate(url,UriKind.Absolute,out var uri)||uri.Scheme!="https")throw new InvalidDataException();
        var response=await transport.SendAsync(uri,"GET",null,token);
        if(response.StatusCode!=200||!ValidPng(response.Body,skin))throw new InvalidDataException();
        return response.Body;
    }
    public static bool ValidPng(byte[] bytes,bool skin)
    {
        if(bytes.Length<33||bytes.Length>256*1024||!bytes.AsSpan(0,8).SequenceEqual(new byte[]{137,80,78,71,13,10,26,10})||!bytes.AsSpan(12,4).SequenceEqual("IHDR"u8))return false;
        var width=BinaryPrimitives.ReadInt32BigEndian(bytes.AsSpan(16,4));var height=BinaryPrimitives.ReadInt32BigEndian(bytes.AsSpan(20,4));
        if(!(skin?width==64&&(height==32||height==64):width>0&&width<=512&&height>0&&height<=512))return false;
        var offset=8;var image=false;
        while(offset<=bytes.Length-12)
        {
            var length=BinaryPrimitives.ReadInt32BigEndian(bytes.AsSpan(offset,4));
            if(length<0||length>bytes.Length-offset-12)return false;
            var crc=new ICSharpCode.SharpZipLib.Checksum.Crc32();crc.Update(new ArraySegment<byte>(bytes,offset+4,length+4));
            if((uint)crc.Value!=BinaryPrimitives.ReadUInt32BigEndian(bytes.AsSpan(offset+8+length,4)))return false;
            if(bytes.AsSpan(offset+4,4).SequenceEqual("IDAT"u8))image=true;
            if(bytes.AsSpan(offset+4,4).SequenceEqual("IEND"u8))return image&&length==0&&offset+12==bytes.Length;
            offset+=length+12;
        }
        return false;
    }
    private sealed record TextureSnapshot(byte[]? Skin,byte[]? Cape,DateTimeOffset Updated);
    private static async Task<TextureSnapshot?> CachedSnapshot(string path,CancellationToken token)
    {
        try
        {
            if(!File.Exists(path)||new FileInfo(path).Length>768*1024)return null;
            var data=JsonSerializer.Deserialize<TextureSnapshot>(await File.ReadAllBytesAsync(path,token));
            return data is not null&&(data.Skin is null||ValidPng(data.Skin,true))&&(data.Cape is null||ValidPng(data.Cape,false))?data:null;
        }
        catch(Exception error) when(error is IOException or JsonException or UnauthorizedAccessException){return null;}
    }
    private static async Task SaveSnapshot(string path,TextureSnapshot data,CancellationToken token)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);var temporary=path+".pending";
        // One atomic snapshot includes absent textures, so removed skins cannot reappear from stale files.
        await File.WriteAllBytesAsync(temporary,JsonSerializer.SerializeToUtf8Bytes(data),token);File.Move(temporary,path,true);
    }
}
