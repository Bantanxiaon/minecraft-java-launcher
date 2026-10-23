using System.Runtime.InteropServices;
using System.Security.Cryptography;
using SHLauncher.Contracts.Accounts;
namespace SHLauncher.Platform;
// Narrow Windows adapter; current-user DPAPI only, never machine-wide protection or plaintext fallback.
public sealed class WindowsCredentialProtector:ICredentialProtector
{
    public bool IsSupported=>OperatingSystem.IsWindows();
    public byte[] Protect(ReadOnlySpan<byte> input)=>Transform(input,true);
    public byte[] Unprotect(ReadOnlySpan<byte> input)=>Transform(input,false);
    private byte[] Transform(ReadOnlySpan<byte> input,bool protect)
    {
        if(!IsSupported)throw new AccountException(AccountError.Unavailable,"当前平台尚未接入系统安全凭据存储。");
        if(input.Length>65536)throw new AccountException(AccountError.Storage,"凭据记录超过安全大小上限。");
        var copy=input.ToArray();var memory=Marshal.AllocHGlobal(Math.Max(1,copy.Length));Blob output=default;
        try
        {
            Marshal.Copy(copy,0,memory,copy.Length);var source=new Blob{Length=copy.Length,Data=memory};
            var success=protect?CryptProtectData(ref source,null,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,1,out output):CryptUnprotectData(ref source,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,1,out output);
            if(!success||output.Length is <0 or >131072)throw new AccountException(AccountError.Storage,"无法使用当前系统用户读取或保护登录凭据。");
            var result=new byte[output.Length];Marshal.Copy(output.Data,result,0,result.Length);return result;
        }
        finally
        {
            CryptographicOperations.ZeroMemory(copy);
            for(var i=0;i<input.Length;i++)Marshal.WriteByte(memory,i,0);
            Marshal.FreeHGlobal(memory);
            if(output.Data!=IntPtr.Zero){if(!protect)for(var i=0;i<output.Length;i++)Marshal.WriteByte(output.Data,i,0);LocalFree(output.Data);}
        }
    }
    [StructLayout(LayoutKind.Sequential)] private struct Blob{internal int Length;internal IntPtr Data;}
    [DllImport("crypt32.dll",CharSet=CharSet.Unicode,SetLastError=true)] [return:MarshalAs(UnmanagedType.Bool)]
    private static extern bool CryptProtectData(ref Blob input,string? description,IntPtr entropy,IntPtr reserved,IntPtr prompt,int flags,out Blob output);
    [DllImport("crypt32.dll",CharSet=CharSet.Unicode,SetLastError=true)] [return:MarshalAs(UnmanagedType.Bool)]
    private static extern bool CryptUnprotectData(ref Blob input,IntPtr description,IntPtr entropy,IntPtr reserved,IntPtr prompt,int flags,out Blob output);
    [DllImport("kernel32.dll")] private static extern IntPtr LocalFree(IntPtr pointer);
}
