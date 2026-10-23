using System.Text.Json;
using SHLauncher.Contracts.Install;
namespace SHLauncher.Infrastructure.Install;
public sealed class JsonInstallationTaskStore(string directory) : IInstallationTaskStore
{
    public async Task<IReadOnlyList<InstallationTaskRecord>> LoadAsync(CancellationToken token)
    {
        if (!Directory.Exists(directory)) return [];
        var records = new List<InstallationTaskRecord>();
        try
        {
            foreach (var file in Directory.EnumerateFiles(directory, "*.json").OrderByDescending(File.GetLastWriteTimeUtc).Take(64))
            {
                if (new FileInfo(file).Length > 256 * 1024) throw new InvalidDataException("Task record exceeds size limit.");
                using var stream = File.OpenRead(file);
                var record = await JsonSerializer.DeserializeAsync<InstallationTaskRecord>(stream, cancellationToken: token).ConfigureAwait(false);
                if (record?.Recipe is not { } recipe || record.Snapshot?.Progress is null || recipe.Id == Guid.Empty || recipe.Id != record.Snapshot.Id || !Enum.IsDefined(recipe.Loader) || !Enum.IsDefined(record.Snapshot.State) || string.IsNullOrWhiteSpace(recipe.MinecraftVersion) || recipe.LoaderVersion is null || string.IsNullOrWhiteSpace(recipe.DisplayName) || recipe.DisplayName.Length > 128 || Path.GetFileNameWithoutExtension(file) != recipe.Id.ToString("N")) throw new InvalidDataException("Task identity does not match record.");
                records.Add(record);
            }
            return records;
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or InvalidDataException or JsonException)
        { throw new InstallationPersistenceException("任务记录无法读取，原记录已保留。", e); }
    }
    public async Task SaveAsync(InstallationTaskRecord record, CancellationToken token)
    {
        try
        {
            Directory.CreateDirectory(directory);
            var destination = Path.Combine(directory, record.Recipe.Id.ToString("N") + ".json");
            var part = destination + "." + Guid.NewGuid().ToString("N") + ".part";
            await using (var stream = new FileStream(part, FileMode.CreateNew, FileAccess.Write, FileShare.None, 8192, true))
            { await JsonSerializer.SerializeAsync(stream, record, cancellationToken: token).ConfigureAwait(false); await stream.FlushAsync(token).ConfigureAwait(false); stream.Flush(true); }
            token.ThrowIfCancellationRequested(); File.Move(part, destination, true);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        { throw new InstallationPersistenceException("任务记录无法保存。", e); }
    }
}
