using SHLauncher.Contracts.Downloads;
namespace SHLauncher.Contracts.Install;
public sealed record InstallationRecipe(Guid Id, string MinecraftVersion, LoaderType Loader, string LoaderVersion, [property: System.Text.Json.Serialization.JsonPropertyName("DirectoryName")] string DisplayName, Guid InstanceKey = default, string? ArchivePath = null, string? ArchiveSha256 = null);
public sealed record InstallationTaskRecord(InstallationRecipe Recipe, DownloadTaskSnapshot Snapshot, InstallResult? Result = null);
public interface IInstallationBackend
{
    Task<InstallResult> InstallAsync(InstallationRecipe recipe, IProgress<InstallProgress>? progress, CancellationToken token);
}
public interface IInstallationTaskStore
{
    Task<IReadOnlyList<InstallationTaskRecord>> LoadAsync(CancellationToken token);
    Task SaveAsync(InstallationTaskRecord record, CancellationToken token);
}
public sealed class InstallationPersistenceException(string message, Exception? inner = null) : Exception(message, inner);

public sealed class InstallationPreconditionException(string code,string message):Exception(message) { public string Code {get;}=code; }
