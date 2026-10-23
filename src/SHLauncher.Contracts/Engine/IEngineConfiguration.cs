namespace SHLauncher.Contracts.Engine;
public interface IEngineConfiguration
{
    IGameLogService GameLogs { get; }
    bool IsMock { get; }
    string Root { get; }
    string ManualJava { get; set; }
    Task<string> UseRootAsync(string root, CancellationToken token);
    Task<string> UseMockAsync(CancellationToken token);
}
