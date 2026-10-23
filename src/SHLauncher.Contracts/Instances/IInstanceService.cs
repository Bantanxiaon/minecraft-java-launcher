using SHLauncher.Contracts.Engine;
namespace SHLauncher.Contracts.Instances;
public interface IInstanceService
{
    Task<IReadOnlyList<InstanceInfo>> ScanAsync(string root, CancellationToken cancellationToken);
}
public sealed record InstanceLocation(string Root, string VersionDirectory, string GameDirectory,
    string? LoaderVersion = null);
