namespace SHLauncher.Contracts.Accounts;
public sealed record TextureCapabilities(bool CanReadSkin,bool CanUploadSkin,bool CanReadCape,bool CanUploadCape,bool CanSelectCape);
public sealed record ProfileTextures(byte[]? Skin,byte[]? Cape,TextureCapabilities Capabilities,bool FromCache,string Status);
public interface IProfileTextureService
{
    Task<ProfileTextures> ReadAsync(MinecraftAccount account,AuthenticationAuthority? authority,bool force,CancellationToken token);
}
