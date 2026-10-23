using System.Security.Cryptography;
using SHLauncher.Contracts.Accounts;
using SHLauncher.Infrastructure.Install;
namespace SHLauncher.Infrastructure.Accounts;
public sealed class ProtectedCredentialStore(string root,ICredentialProtector protector):ICredentialStore
{
    public bool IsSupported=>protector.IsSupported;
    private string Target(Guid id)
    {if(id==Guid.Empty)throw new AccountException(AccountError.Storage,"账号身份无效。");return ArchivePathPolicy.Resolve(root,"accounts/private/"+id.ToString("N")+".bin");}
    public async Task<byte[]?> ReadAsync(Guid id,CancellationToken token)
    {
        var target=Target(id);if(!File.Exists(target))return null;
        if(new FileInfo(target).Length>131072)throw new AccountException(AccountError.Storage,"登录凭据记录无效。");
        byte[]? clear=null;
        try
        {
            clear=protector.Unprotect(await File.ReadAllBytesAsync(target,token).ConfigureAwait(false));
            if(clear.Length<16||new Guid(clear.AsSpan(0,16))!=id)throw new AccountException(AccountError.Storage,"登录凭据不属于该账号。");
            return clear.Length==16?null:clear[16..];
        }
        catch(Exception error) when(error is IOException or UnauthorizedAccessException){throw new AccountException(AccountError.Storage,"无法读取受保护的登录凭据。");}
        finally{if(clear is not null)CryptographicOperations.ZeroMemory(clear);}
    }
    public async Task WriteAsync(Guid id,ReadOnlyMemory<byte> credential,CancellationToken token)
    {
        var target=Target(id);if(credential.Length>32768)throw new AccountException(AccountError.Storage,"登录凭据过大。");
        var clear=new byte[16+credential.Length];id.TryWriteBytes(clear);credential.Span.CopyTo(clear.AsSpan(16));
        byte[] encrypted;
        try{encrypted=protector.Protect(clear);}finally{CryptographicOperations.ZeroMemory(clear);}
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(target)!);var part=target+"."+Guid.NewGuid().ToString("N")+".part";
            await using(var file=new FileStream(part,FileMode.CreateNew,FileAccess.Write,FileShare.None,8192,true))
            {await file.WriteAsync(encrypted,token).ConfigureAwait(false);await file.FlushAsync(token).ConfigureAwait(false);file.Flush(true);}
            token.ThrowIfCancellationRequested();File.Move(part,target,true);
        }
        catch(Exception error) when(error is IOException or UnauthorizedAccessException){throw new AccountException(AccountError.Storage,"无法保存受保护的登录凭据。");}
    }
    // An encrypted empty tombstone replaces credentials atomically; no recoverable plaintext file is ever produced.
    public Task RemoveAsync(Guid id,CancellationToken token)=>WriteAsync(id,ReadOnlyMemory<byte>.Empty,token);
}
