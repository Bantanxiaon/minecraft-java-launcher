namespace SHLauncher.Contracts.Install;
public interface IXmlMetadataSource { Task<Stream> OpenReadAsync(Uri uri, Guid sessionId, CancellationToken token); }
