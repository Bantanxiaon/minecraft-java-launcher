using SHLauncher.Contracts.Install;
using SHLauncher.Application.Install;
using SHLauncher.Contracts.Downloads;
using SHLauncher.UI.Presentation;
namespace SHLauncher.UI.ViewModels;
public sealed class DownloadCardViewModel : ObservableModel
{
    public InstallationTaskRecord Record { get; private set; }
    public void Update(InstallationTaskRecord record)
    {
        if(Record==record)return;
        Record = record;
        foreach (var name in new[] { nameof(Title),nameof(DisplayName),nameof(EnvironmentLabel),nameof(SourceSummary), nameof(Stage), nameof(Counts), nameof(Transfer), nameof(Problem), nameof(IsIndeterminate), nameof(Progress), nameof(Details),nameof(IsActive),nameof(IsFailed),nameof(IsCancelled),nameof(CanRetry),nameof(CanOpen),nameof(HasProblem),nameof(StateLabel),nameof(IsCompleted) }) Raise(name);
        CancelCommand.Refresh(); RetryCommand.Refresh(); OpenCommand.Refresh();
    }
    public string Title => Record.Snapshot.Title;
    public string DisplayName=>Record.Recipe.DisplayName;
    public string EnvironmentLabel=>"Minecraft "+Record.Recipe.MinecraftVersion+" · "+Record.Recipe.Loader+(string.IsNullOrEmpty(Record.Recipe.LoaderVersion)?"":" "+Record.Recipe.LoaderVersion);
    public bool IsActive => Record.Snapshot.State is not (DownloadTaskState.Completed or DownloadTaskState.Failed or DownloadTaskState.Cancelled);
    public bool HasProblem=>!string.IsNullOrWhiteSpace(Problem);
    public bool IsFailed => Record.Snapshot.State==DownloadTaskState.Failed;
    public bool IsCancelled => Record.Snapshot.State==DownloadTaskState.Cancelled;
    public bool CanRetry => IsFailed||IsCancelled;
    public bool CanOpen => IsCompleted && Record.Result is not null;
    public string StateLabel => Record.Snapshot.State switch {
        DownloadTaskState.Queued=>"排队中",DownloadTaskState.Resolving=>"正在准备",DownloadTaskState.Downloading=>"正在下载",
        DownloadTaskState.Verifying=>"正在校验",DownloadTaskState.Finalizing=>"正在完成",DownloadTaskState.Completed=>"已完成",
        DownloadTaskState.Failed=>"需要处理",DownloadTaskState.Cancelled=>"已取消",_=>"处理中"};

    public string SourceSummary
    {
        get {
            if(Record.Result is {NetworkBytes:0,ReusedFiles:>0})return "来源：已验证缓存";
            var source=_workspace.DownloadEvents(Record.Recipe.Id).LastOrDefault(e=>e.Kind=="complete")?.SourceId;
            return source is null ? "来源信息见技术详情" : source.Contains("BMCL",StringComparison.OrdinalIgnoreCase) ? "来源：可信镜像" : "来源：上游服务";
        }
    }
    public string Stage => Record.Snapshot.Stage ?? Record.Snapshot.State.ToString();
    public string Counts => Record.Result is { } result ? $"{result.FileCount} 个文件已验证" : Record.Snapshot.TotalFiles==0 ? "尚未确定文件数量" : $"当前阶段 {Record.Snapshot.CompletedFiles} / {Record.Snapshot.TotalFiles} 个文件";
    public string Transfer => !IsActive && Record.Result is null ? $"已传输 {Record.Snapshot.Progress.NetworkBytes / 1048576d:0.0} MiB · 已验证文件可在重试时复用" : Record.Result is { } result ? $"共 {result.FileCount} 个文件 · 已复用 {result.ReusedFiles} · 安装文件网络流量 {result.NetworkBytes / 1048576d:0.0} MiB" : $"当前阶段 · {Record.Snapshot.Progress.NetworkBytes / 1048576d:0.0} MiB 网络流量 · {Record.Snapshot.Progress.CompletedBytes / 1048576d:0.0} / {(Record.Snapshot.Progress.TotalBytes is { } total ? (total / 1048576d).ToString("0.0") : "未知")} MiB · {Record.Snapshot.Progress.BytesPerSecond / 1048576d:0.00} MiB/s";
    public string Problem => Record.Snapshot.Problem ?? "";
    public bool IsIndeterminate => Record.Snapshot.State is not (DownloadTaskState.Completed or DownloadTaskState.Failed or DownloadTaskState.Cancelled) && (Record.Snapshot.State == DownloadTaskState.Resolving || Record.Snapshot.Progress.TotalBytes is null or <= 0);
    public bool IsCompleted => Record.Snapshot.State == DownloadTaskState.Completed;
    public double Progress => IsCompleted ? 100 : Record.Snapshot.Progress.TotalBytes is > 0 ? Math.Clamp(100d * Record.Snapshot.Progress.CompletedBytes / Record.Snapshot.Progress.TotalBytes.Value, 0, 100) : 0;
    public string Details => string.Join(Environment.NewLine, _workspace.DownloadEvents(Record.Recipe.Id).TakeLast(40).Select(e => $"{e.Timestamp:HH:mm:ss} · {e.SourceId} · {e.Kind} · {e.Message}"))
        + Environment.NewLine + string.Join(Environment.NewLine, _logs().TakeLast(20));
    private readonly IInstallationWorkspace _workspace;
    private readonly Func<IEnumerable<string>> _logs;
    public AsyncCommand CancelCommand { get; }
    public DelegateCommand RetryCommand { get; }
    public AsyncCommand OpenCommand { get; }
    public DownloadCardViewModel(InstallationTaskRecord record, IInstallationWorkspace workspace, Action<string> message)
    {
        Record = record; _workspace = workspace;
        _logs = () => workspace is SHLauncher.Contracts.Engine.IEngineConfiguration config ? config.GameLogs.Snapshot(record.Recipe.Id).Select(l => l.Message) : [];
        CancelCommand = new(() => workspace.Installations.CancelAsync(record.Recipe.Id), () => Record.Snapshot.State is not (DownloadTaskState.Completed or DownloadTaskState.Failed or DownloadTaskState.Cancelled), e => message("取消未完成：" + e.GetType().Name));
        RetryCommand = new(() => message(workspace.Installations.Retry(record.Recipe.Id) ? "已加入重试任务。" : "当前无法重试。"), () => Record.Snapshot.State is DownloadTaskState.Failed or DownloadTaskState.Cancelled);
        OpenCommand = new(async () => message(await workspace.OpenInstalledAsync(Record.Result!, default)), () => Record.Result is not null && IsCompleted, e => message("实例未打开：" + e.GetType().Name));
    }
}
