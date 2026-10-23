using SHLauncher.Contracts.Accounts;
namespace SHLauncher.Application.Accounts;
public static class OfflineIdentity
{
    public static void ValidateName(string name)
    {
        if(name.Length is <3 or >16||name.Any(c=>!(c is >= 'a' and <= 'z' or >= 'A' and <= 'Z' or >= '0' and <= '9' or '_')))
            throw new AccountException(AccountError.InvalidInput,"游戏内名称须为 3–16 位英文字母、数字或下划线。");
    }
    // Adapted from XMCL offlineUser.ts (MIT). Java nameUUIDFromBytes("OfflinePlayer:" + exact-case name).
    public static Guid Uuid(string name)
    {
        ValidateName(name);return SHLauncher.Core.Accounts.OfflineUuid.ForName(name);
    }
}
