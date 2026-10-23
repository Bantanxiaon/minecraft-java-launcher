using System.Security.Cryptography;
using System.Text.Json;
using SHLauncher.Contracts.Accounts;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Install;
using SHLauncher.Infrastructure.Install;
namespace SHLauncher.Infrastructure.Accounts;
public sealed class AuthlibInjectorService(IDownloadManager downloads,IJsonMetadataSource metadata,string workspace):IAuthenticationAgentService
{
    private readonly SemaphoreSlim _serial=new(1);
    public async Task<AuthenticationAgent> PrepareAsync(CancellationToken token)
    {
        await _serial.WaitAsync(token).ConfigureAwait(false);
        try
        {
            var root=ArchivePathPolicy.Resolve(workspace,"shared/authlib");Directory.CreateDirectory(root);
            var descriptor=ArchivePathPolicy.Resolve(root,"current.json");
            if(File.Exists(descriptor)&&new FileInfo(descriptor).Length<=16384)
            {
                try
                {
                    var saved=JsonSerializer.Deserialize<Artifact>(await File.ReadAllBytesAsync(descriptor,token).ConfigureAwait(false));
                    if(saved is not null&&Valid(saved))
                    {
                        var cached=ArchivePathPolicy.Resolve(root,saved.Sha256+".jar");
                        if(await Verified(cached,saved.Sha256,token).ConfigureAwait(false))return new(cached,saved.Sha256);
                    }
                }
                catch(Exception e) when(e is JsonException or IOException){/* Re-resolve corrupt metadata; never trust unverified binaries. */}
            }
            await using var stream=await metadata.OpenReadAsync(new("https://authlib-injector.yushi.moe/artifact/latest.json"),Guid.NewGuid(),token).ConfigureAwait(false);
            using var json=await JsonDocument.ParseAsync(stream,new(){MaxDepth=16},token).ConfigureAwait(false);
            var value=json.RootElement;
            var artifact=new Artifact(value.GetProperty("download_url").GetString()!,value.GetProperty("checksums").GetProperty("sha256").GetString()!);
            if(!Valid(artifact))throw new InvalidDataException();
            var target=ArchivePathPolicy.Resolve(root,artifact.Sha256+".jar");
            var result=await downloads.DownloadAsync(new(Guid.NewGuid(),"authlib-injector",root,
                [new(target,null,null,artifact.Sha256,[new("authlib-injector",new(artifact.Url))])]),null,token).ConfigureAwait(false);
            token.ThrowIfCancellationRequested();
            if(!result.Succeeded||!await Verified(target,artifact.Sha256,token).ConfigureAwait(false))throw new IOException();
            var part=descriptor+".part";await File.WriteAllBytesAsync(part,JsonSerializer.SerializeToUtf8Bytes(artifact),token).ConfigureAwait(false);
            token.ThrowIfCancellationRequested();File.Move(part,descriptor,true);
            return new(target,artifact.Sha256);
        }
        catch(Exception e) when(e is IOException or JsonException or InvalidOperationException or KeyNotFoundException or ArgumentException)
        {throw new AccountException(AccountError.Unavailable,"认证启动组件未准备完成，请检查网络后重试；已校验的组件可离线复用。");}
        finally{_serial.Release();}
    }
    private static bool Valid(Artifact value)=>Uri.TryCreate(value.Url,UriKind.Absolute,out var uri)&&uri.Scheme=="https"&&uri.Host=="authlib-injector.yushi.moe"&&uri.Port==443&&uri.UserInfo.Length==0&&uri.Query.Length==0&&uri.Fragment.Length==0&&uri.AbsolutePath.StartsWith("/artifact/",StringComparison.Ordinal)&&value.Sha256 is {Length:64}&&value.Sha256.All(Uri.IsHexDigit);
    private static async Task<bool> Verified(string path,string hash,CancellationToken token)
    {if(!File.Exists(path))return false;await using var file=File.OpenRead(path);return Convert.ToHexString(await SHA256.HashDataAsync(file,token).ConfigureAwait(false)).Equals(hash,StringComparison.OrdinalIgnoreCase);}
    private sealed record Artifact(string Url,string Sha256);
}
