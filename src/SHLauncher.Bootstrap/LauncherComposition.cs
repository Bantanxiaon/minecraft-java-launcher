using SHLauncher.Application.Launching;
using SHLauncher.Application.Runtime;
using SHLauncher.Contracts.Engine;
using SHLauncher.Engine.CmlLib;
using SHLauncher.Infrastructure.Mocks;
using SHLauncher.Infrastructure.Runtime;
using SHLauncher.Infrastructure.Accounts;
using SHLauncher.Infrastructure.Logging;
namespace SHLauncher.Bootstrap;
public sealed partial class LauncherComposition : IEngineConfiguration, SHLauncher.Application.Install.IInstallationWorkspace, SHLauncher.Application.Accounts.IAccountWorkspace
{
    public MemoryLauncherLog LauncherLog { get; } = new();
    public IGameLogService GameLogs { get; private set; } = new GameLogService();
    public LaunchCoordinator Coordinator { get; }
    public bool IsMock { get; private set; } = true;
    public string Root { get; private set; } = "";
    public string ManualJava { get; set; } = "";
    public static string DefaultDataDirectory() => ApplicationDataPaths.Resolve();
    private readonly string _workspace;
    public LauncherComposition(string workspace, string scenario = "Success")
    { _workspace = workspace; InitializeInstallations(); Coordinator = new(new MockLauncherEngine(new() { Scenario = Enum.TryParse<MockScenario>(scenario, true, out var selected) ? selected : MockScenario.Success }), LauncherLog); InitializeAccounts(); }
    public async Task<string> UseRootAsync(string root, CancellationToken token)
    {
        var engine = new CmlLibLauncherEngine(new CmlLibInstanceService(), new JavaRuntimeService(), new JavaSelectionService(),
            Accounts, GameLogs, root, _workspace, [], manualJavaProvider: () => ManualJava, authenticationAgent: _authenticationAgent, memory: Memory);
        try
        {
            if (!await Coordinator.ChangeEngineAsync(_instanceAccess.Guard(engine, root), token).ConfigureAwait(false)) return "游戏任务进行中，请结束后再更换目录。";
            Root = root; IsMock = false;_managedLibrary=false;SaveLibrarySelection();
            return $"已读取 {Coordinator.Instances.Count} 个实例。启动会使用独立游戏目录；关闭启动器前会确认是否结束游戏。";
        }
        catch (LauncherEngineException e) { return e.Error.Message; }
    }
    public async Task<string> UseMockAsync(CancellationToken token)
    {
        if (!await Coordinator.ChangeEngineAsync(new MockLauncherEngine(new()), token).ConfigureAwait(false)) return "任务进行中，暂时无法切换。";
        IsMock = true; return "已切换演示模式。";
    }
}
