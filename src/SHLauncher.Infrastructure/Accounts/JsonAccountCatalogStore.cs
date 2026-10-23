using System.Text.Json;
using SHLauncher.Contracts.Accounts;
using SHLauncher.Infrastructure.Install;
namespace SHLauncher.Infrastructure.Accounts;
public sealed class JsonAccountCatalogStore(string dataRoot):IAccountCatalogStore
{
    private string Target=>ArchivePathPolicy.Resolve(dataRoot,"accounts/accounts.json");
    public async Task<AccountCatalog> LoadAsync(CancellationToken token)
    {
        try
        {
            if(!File.Exists(Target))return new([],[],null);
            if(new FileInfo(Target).Length>1024*1024)throw new InvalidDataException();
            await using var stream=File.OpenRead(Target);
            var result=await JsonSerializer.DeserializeAsync<AccountCatalog>(stream,cancellationToken:token).ConfigureAwait(false)??throw new InvalidDataException();
            Validate(result);return result;
        }
        catch(Exception error) when(error is IOException or JsonException or InvalidDataException or UnauthorizedAccessException or ArgumentException)
        {throw new AccountException(AccountError.Storage,"账号记录无法读取，原文件已保留。请检查数据目录或恢复备份。");}
    }
    public async Task SaveAsync(AccountCatalog catalog,CancellationToken token)
    {
        Validate(catalog);
        try
        {
            var target=Target;
            if(File.Exists(target)){await LoadAsync(token).ConfigureAwait(false);File.Copy(target,target+".bak",true);}
            Directory.CreateDirectory(Path.GetDirectoryName(target)!);
            var part=target+"."+Guid.NewGuid().ToString("N")+".part";
            await using(var file=new FileStream(part,FileMode.CreateNew,FileAccess.Write,FileShare.None,8192,true))
            {await JsonSerializer.SerializeAsync(file,catalog,cancellationToken:token).ConfigureAwait(false);await file.FlushAsync(token).ConfigureAwait(false);file.Flush(true);}
            token.ThrowIfCancellationRequested();File.Move(part,target,true);
        }
        catch(Exception error) when(error is IOException or UnauthorizedAccessException)
        {throw new AccountException(AccountError.Storage,"无法保存账号记录，请检查数据目录权限和剩余空间。");}
    }
    private static void Validate(AccountCatalog value)
    {
        if(value.SchemaVersion!=1||value.Accounts is null||value.Authorities is null||value.Accounts.Count>100||value.Authorities.Count>100||
            value.Accounts.Any(a=>a is null)||value.Authorities.Any(a=>a is null))throw new AccountException(AccountError.Storage,"账号记录结构无效，原文件已保留。");
        if(value.Accounts.GroupBy(a=>a.AccountId).Any(g=>g.Count()>1)||value.Authorities.GroupBy(a=>a.ProviderId).Any(g=>g.Count()>1))throw new AccountException(AccountError.Storage,"账号记录存在重复身份。");
        foreach(var authority in value.Authorities)
            if(!Guid.TryParseExact(authority.ProviderId,"N",out _)||string.IsNullOrWhiteSpace(authority.DisplayName)||authority.DisplayName.Length>128||authority.ApiRoot is null||!authority.ApiRoot.IsAbsoluteUri||authority.ApiRoot.Scheme!="https"||authority.ApiRoot.UserInfo.Length!=0||authority.ApiRoot.Query.Length!=0||authority.ApiRoot.Fragment.Length!=0)
                throw new AccountException(AccountError.Storage,"认证服务记录无效。");
        foreach(var account in value.Accounts)
            if(account.AccountId==Guid.Empty||account.ProfileId==Guid.Empty||!Enum.IsDefined(account.AccountType)||!Enum.IsDefined(account.AuthenticationState)||string.IsNullOrWhiteSpace(account.ProfileName)||account.ProfileName.Length>64||string.IsNullOrWhiteSpace(account.ProviderId)||account.ProviderId.Length>128||string.IsNullOrWhiteSpace(account.ProviderDisplayName)||account.ProviderDisplayName.Length>128||
                account.AccountType==MinecraftAccountType.Yggdrasil&&!value.Authorities.Any(a=>a.ProviderId==account.ProviderId))throw new AccountException(AccountError.Storage,"账号身份记录无效。");
        if(value.ActiveAccountId is not null&&!value.Accounts.Any(a=>a.AccountId==value.ActiveAccountId))throw new AccountException(AccountError.Storage,"当前账号指向不存在的身份。");
    }
}
