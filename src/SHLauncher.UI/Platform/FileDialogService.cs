using Avalonia.Controls;
using Avalonia.Platform.Storage;
using SHLauncher.Contracts.Platform;
namespace SHLauncher.UI.Platform;
internal sealed class FileDialogService(TopLevel topLevel) : IFileDialogService
{
    public async Task<string?> PickModpackAsync(CancellationToken token)
    {
        token.ThrowIfCancellationRequested();
        if (!topLevel.StorageProvider.CanOpen) return null;
        var items = await topLevel.StorageProvider.OpenFilePickerAsync(new() { Title = "导入整合包", AllowMultiple = false,
            FileTypeFilter = [new FilePickerFileType("CurseForge 整合包") { Patterns = ["*.zip"] }] });
        token.ThrowIfCancellationRequested(); return items.FirstOrDefault()?.TryGetLocalPath();
    }
    public async Task<string?> PickFolderAsync(CancellationToken token)
    {
        token.ThrowIfCancellationRequested();
        if (!topLevel.StorageProvider.CanPickFolder) return null;
        var items = await topLevel.StorageProvider.OpenFolderPickerAsync(new() { Title = "选择 Minecraft 根目录", AllowMultiple = false });
        token.ThrowIfCancellationRequested();
        return items.FirstOrDefault()?.TryGetLocalPath();
    }
    public async Task<string?> PickJavaAsync(CancellationToken token)
    {
        token.ThrowIfCancellationRequested();
        if (!topLevel.StorageProvider.CanOpen) return null;
        var items = await topLevel.StorageProvider.OpenFilePickerAsync(new() { Title = "选择现有 Java 可执行文件", AllowMultiple = false });
        token.ThrowIfCancellationRequested();
        return items.FirstOrDefault()?.TryGetLocalPath();
    }
}
