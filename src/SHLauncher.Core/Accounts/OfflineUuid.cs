using System.Security.Cryptography;
using System.Text;
namespace SHLauncher.Core.Accounts;
public static class OfflineUuid
{
    // XMCL offlineUser.ts (MIT): Minecraft's case-sensitive OfflinePlayer name UUID, version 3 / IETF variant.
    public static Guid ForName(string name)
    {
        var bytes=MD5.HashData(Encoding.UTF8.GetBytes("OfflinePlayer:"+name));
        bytes[6]=(byte)((bytes[6]&15)|0x30);bytes[8]=(byte)((bytes[8]&63)|0x80);
        return Guid.ParseExact(Convert.ToHexString(bytes),"N");
    }
}
