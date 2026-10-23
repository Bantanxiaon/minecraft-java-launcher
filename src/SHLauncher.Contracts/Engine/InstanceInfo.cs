namespace SHLauncher.Contracts.Engine;

public sealed record InstanceInfo(string Id, string Name, string GameVersion, string Loader,
    int JavaMajor, int MemoryMb, string LastPlayed, string Description, string Artwork,
    SHLauncher.Contracts.Instances.InstanceLocation? Location = null, LauncherError? Problem = null, bool IsLaunchable = true);
