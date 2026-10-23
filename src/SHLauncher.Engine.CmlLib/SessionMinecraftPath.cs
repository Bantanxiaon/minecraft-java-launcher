using CmlLib.Core;
namespace SHLauncher.Engine.CmlLib;
internal sealed class SessionMinecraftPath : MinecraftPath
{
    private readonly string _folderId, _metadataId, _nativeDirectory;
    internal SessionMinecraftPath(string root, string folderId, string metadataId, string gameDirectory, string nativeDirectory) : base(root)
    { _folderId = folderId; _metadataId = metadataId; _nativeDirectory = nativeDirectory; BasePath = gameDirectory; }
    public override string GetVersionJarPath(string id) => base.GetVersionJarPath(id == _metadataId ? _folderId : id);
    public override string GetNativePath(string id) => _nativeDirectory;
}
