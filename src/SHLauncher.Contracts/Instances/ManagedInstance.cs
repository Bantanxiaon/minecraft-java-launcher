using SHLauncher.Contracts.Install;
namespace SHLauncher.Contracts.Instances;
public sealed record ManagedInstance(Guid Id, string DisplayName, string MinecraftVersion, LoaderType Loader, string LoaderVersion, string ProfileId, int SchemaVersion = 1);
