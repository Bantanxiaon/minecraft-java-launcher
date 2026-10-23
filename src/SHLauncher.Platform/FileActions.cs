using System.Diagnostics;
using Microsoft.VisualBasic.FileIO;
using SHLauncher.Contracts.Content;
namespace SHLauncher.Platform;
public sealed class RecycleService:IRecycleService
{
    public Task RecycleAsync(string path,CancellationToken token)=>Task.Run(()=>
    {
        token.ThrowIfCancellationRequested();if(!OperatingSystem.IsWindows())throw new PlatformNotSupportedException("此平台回收站暂未接入，文件未删除。");
        FileSystem.DeleteFile(path,UIOption.OnlyErrorDialogs,RecycleOption.SendToRecycleBin,UICancelOption.ThrowException);
    },token);
}
public sealed class PathOpener:IPathOpener
{
    public void OpenFolder(string directory)
    {if(!Path.IsPathFullyQualified(directory)||!Directory.Exists(directory))throw new DirectoryNotFoundException();Process.Start(new ProcessStartInfo(directory){UseShellExecute=true});}
}
