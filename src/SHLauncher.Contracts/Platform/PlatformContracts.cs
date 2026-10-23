namespace SHLauncher.Contracts.Platform;

// Reserved boundaries only. No platform implementation or runtime paths exist yet.
public interface IPlatformService;
public interface IPathService;
public interface IFileDialogService
{
    Task<string?> PickModpackAsync(CancellationToken token) => Task.FromResult<string?>(null);
    Task<string?> PickFolderAsync(CancellationToken token);
    Task<string?> PickJavaAsync(CancellationToken token);
}
public interface ICredentialStore;
public interface IProcessService;
public interface ISystemInfoService;
public interface IClipboardService;
public interface INotificationService;
public interface IWindowEffectService;
public interface IFileAssociationService;
