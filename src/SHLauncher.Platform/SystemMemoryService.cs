using System.Runtime.InteropServices;
using SHLauncher.Contracts.Runtime;
namespace SHLauncher.Platform;
public sealed class SystemMemoryService:ISystemMemoryService
{
    [StructLayout(LayoutKind.Sequential)]private struct Status{public uint Length,Load;public ulong Total,Available,TotalPage,AvailablePage,TotalVirtual,AvailableVirtual,Extended;}
    [DllImport("kernel32.dll",SetLastError=true)][return:MarshalAs(UnmanagedType.Bool)]private static extern bool GlobalMemoryStatusEx(ref Status status);
    public SystemMemory Read()
    {
        if(!OperatingSystem.IsWindows())throw new PlatformNotSupportedException("当前平台尚未接入物理内存统计。");
        var status=new Status{Length=(uint)Marshal.SizeOf<Status>()};
        if(!GlobalMemoryStatusEx(ref status))throw new IOException("系统内存信息读取失败。");
        return new((long)(status.Total/1048576),(long)(status.Available/1048576));
    }
}
