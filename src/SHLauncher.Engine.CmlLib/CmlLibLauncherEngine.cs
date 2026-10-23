using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Diagnostics;
using CmlLib.Core.Auth;
using CmlLib.Core.ProcessBuilder;
using CmlLib.Core.Rules;
using SHLauncher.Contracts.Accounts;
using SHLauncher.Contracts.Engine;
using SHLauncher.Contracts.Instances;
using SHLauncher.Contracts.Runtime;
using SHLauncher.Core.Launching;
namespace SHLauncher.Engine.CmlLib;
public sealed class CmlLibLauncherEngine(IInstanceService instances, IJavaRuntimeService java, IJavaSelectionService selection,
    IAccountProvider accounts, IGameLogService logs, string root, string workspace, IReadOnlyList<string> javaCandidates,
    string? manualJava = null, Func<string?>? manualJavaProvider = null, bool usePersistentGameData = false, string? gameDirectoryOverride = null, IAuthenticationAgentService? authenticationAgent = null, IGameMemoryService? memory = null, string? memoryInstanceId = null) : ILauncherEngine
{
    private int _active;
    public Task<IReadOnlyList<InstanceInfo>> GetInstancesAsync(CancellationToken cancellationToken) => instances.ScanAsync(root, cancellationToken);
    public async IAsyncEnumerable<EngineUpdate> LaunchAsync(LaunchRequest request, [EnumeratorCancellation] CancellationToken cancellationToken)
    {
        if (Interlocked.CompareExchange(ref _active, 1, 0) != 0) throw new LauncherEngineException(new(LauncherErrorCode.LaunchFailed, "已有任务运行", "请等待当前游戏退出。", null, false));
        var id = request.SessionId == Guid.Empty ? Guid.NewGuid() : request.SessionId;
        try
        {
            await using var sequence = RunAsync(request, id, cancellationToken).GetAsyncEnumerator(cancellationToken);
            while (true)
            {
                bool next;
                try { next = await sequence.MoveNextAsync().ConfigureAwait(false); }
                catch (OperationCanceledException) { logs.Write(id, GameLogLevel.Information, "lifecycle", "Operation cancelled or game stopped; process cleanup completed."); throw; }
                catch (LauncherEngineException) { throw; }
                catch (AccountException e) { throw new LauncherEngineException(new(LauncherErrorCode.LaunchFailed,"账号验证未完成",e.Message,null,true)); }
                catch (Exception e)
                {
                    logs.Write(id, GameLogLevel.Error, "engine", e.GetType().Name);
                    throw new LauncherEngineException(new(LauncherErrorCode.LaunchFailed, "启动未完成", "本地启动检查失败，请查看会话日志。", e.GetType().Name, true));
                }
                if (!next) break;
                yield return sequence.Current;
            }
        }
        finally { Volatile.Write(ref _active, 0); }
    }
    private async IAsyncEnumerable<EngineUpdate> RunAsync(LaunchRequest request, Guid id, [EnumeratorCancellation] CancellationToken token)
    {
        await Task.Yield();
        yield return Preparing("准备实例");
        var catalog = await GetInstancesAsync(token).ConfigureAwait(false);
        var instance = catalog.FirstOrDefault(x => x.Id == request.InstanceId) ?? throw new LauncherEngineException(new(LauncherErrorCode.InstanceNotFound, "找不到实例", "请重新扫描游戏目录。", null, false));
        if (!instance.IsLaunchable) throw new LauncherEngineException(instance.Problem!);
        yield return Preparing("检查 Java 运行环境");
        var requestedJava = manualJavaProvider?.Invoke() ?? manualJava;
        var candidates = javaCandidates.Concat(string.IsNullOrWhiteSpace(requestedJava) ? [] : new[] { requestedJava });
        var runtimes = await java.DetectAsync(candidates, token).ConfigureAwait(false);
        var architecture = RuntimeInformation.OSArchitecture.ToString().ToLowerInvariant();
        var chosen = selection.Select(instance.JavaMajor, architecture, string.IsNullOrWhiteSpace(requestedJava) ? null : requestedJava, runtimes);
        if (chosen.Runtime is null) throw new LauncherEngineException(new(chosen.Issue == JavaSelectionIssue.Mismatch ? LauncherErrorCode.JavaVersionMismatch : LauncherErrorCode.RuntimeMissing,
            "Java 运行环境不匹配", chosen.Problem!, null, false));
        logs.Write(id, GameLogLevel.Information, "runtime", $"Java {chosen.Runtime.Version} {chosen.Runtime.Architecture} {chosen.Runtime.Vendor}");
        var version = await LocalVersionSource.ReadAsync(root, instance.Id, token).ConfigureAwait(false);
        try
        {
            var sessionDirectory = PathSafety.EnsureContained(workspace, Path.Combine(workspace, "sessions", id.ToString("N")));
            if (Directory.Exists(sessionDirectory)) throw new InvalidOperationException("Session directory already exists.");
            var gameDirectory = gameDirectoryOverride ?? (usePersistentGameData
                ? PathSafety.EnsureContained(workspace, Path.Combine(workspace, "games", instance.Id))
                : Path.Combine(sessionDirectory, "game"));
            var path = new SessionMinecraftPath(root, instance.Id, version.Id, gameDirectory, Path.Combine(sessionDirectory, "natives"));
            yield return Preparing("检查游戏文件、依赖库与资产");
            var files = await Task.Run(() => CmlLibFileVerifier.PlanAsync(path, version, token), token).ConfigureAwait(false);
            foreach (var file in files) await CmlLibFileVerifier.CheckAsync(path, file, token).ConfigureAwait(false);
            token.ThrowIfCancellationRequested();
            yield return Preparing("准备原生库");
            Directory.CreateDirectory(gameDirectory);
            string natives;
            try { natives = await Task.Run(() => CmlLibNativePreparation.Prepare(path, version, token), token).ConfigureAwait(false); }
            catch (OperationCanceledException) { throw; }
            catch (Exception e) { throw new LauncherEngineException(new(LauncherErrorCode.NativeExtractionError, "原生库准备失败", "无法安全准备原生库，请检查会话日志。", e.GetType().Name, false)); }
            yield return Preparing("构建启动参数");
            var account = await accounts.GetAsync(token).ConfigureAwait(false);
            var memoryMb=instance.MemoryMb;
            if(memory is not null)
            {
                var resolved=await memory.InspectAsync(memoryInstanceId is null?instance:instance with {Id=memoryInstanceId},chosen.Runtime,gameDirectory,token).ConfigureAwait(false);
                if(resolved.Problem is not null)throw new LauncherEngineException(new(LauncherErrorCode.LaunchArgumentError,"游戏内存不可用",resolved.Problem,null,false));
                memoryMb=resolved.ResolvedMb;
                logs.Write(id,GameLogLevel.Information,"memory",$"Resolved heap: {memoryMb} MiB; available: {resolved.System.AvailableMb} MiB; mods: {resolved.ModCount}");
            }
            var option = new MLaunchOption { Path = path, StartVersion = version, JavaPath = chosen.Runtime.ExecutablePath,
                NativesDirectory = natives, MaximumRamMb = memoryMb, GameLauncherName = "SHLN", GameLauncherVersion = "0.3",
                ScreenWidth = 1100, ScreenHeight = 700 };
            await AccountLaunchMapping.ApplyAsync(option,account,authenticationAgent,token).ConfigureAwait(false);
            Process process;
            try { process = new MinecraftProcessBuilder(new RulesEvaluator(), new RulesEvaluatorContext(LauncherOSRule.Current), option).CreateProcess(); }
            catch (Exception e) { throw new LauncherEngineException(new(LauncherErrorCode.LaunchArgumentError, "启动参数无法生成", "请检查实例元数据。", e.GetType().Name, false)); }
            using (process)
            {
                if(memory is not null)
                {
                    var arguments=string.Join(" ",process.StartInfo.ArgumentList)+" "+process.StartInfo.Arguments;
                    var heap=System.Text.RegularExpressions.Regex.Matches(arguments,@"(?<!\S)-Xmx(\d+)([mMgG])(?=\s|$)");
                    if(heap.Count!=1||!long.TryParse(heap[0].Groups[1].Value,out var heapValue)||heapValue*(heap[0].Groups[2].Value.Equals("g",StringComparison.OrdinalIgnoreCase)?1024:1)!=memoryMb)
                        throw new LauncherEngineException(new(LauncherErrorCode.LaunchArgumentError,"内存参数校验失败","实际 JVM 参数与内存策略不一致，已阻止启动。",null,false));
                    logs.Write(id,GameLogLevel.Information,"memory","Verified JVM heap: "+heap[0].Value);
                }
                process.StartInfo.UseShellExecute = false; process.StartInfo.RedirectStandardOutput = true; process.StartInfo.RedirectStandardError = true; process.StartInfo.CreateNoWindow = true;
                foreach (var key in new[] { "JAVA_TOOL_OPTIONS", "_JAVA_OPTIONS", "JDK_JAVA_OPTIONS", "SH_CURSEFORGE_API_KEY", "SH_CURSEFORGE_CONFIG_FILE" }) process.StartInfo.Environment.Remove(key);
                // Per-process temp path stays inside the session, without changing machine settings.
                var temp = Path.Combine(sessionDirectory, "tmp"); Directory.CreateDirectory(temp);
                process.StartInfo.Environment["TEMP"] = temp; process.StartInfo.Environment["TMP"] = temp; process.StartInfo.Environment["TMPDIR"] = temp;
                yield return new(LauncherTaskState.Launching, 0, "正在启动 Java · 关闭启动器会结束当前游戏", true);
                token.ThrowIfCancellationRequested();
                try { process.Start(); }
                catch (Exception e) { throw new LauncherEngineException(new(LauncherErrorCode.ProcessStartFailed, "无法运行 Java", "请重新检测指定的 Java。", e.GetType().Name, false)); }
                logs.Write(id, GameLogLevel.Information, "lifecycle", "Process started: " + process.Id);
                var stdout = Pump(process.StandardOutput, id, "stdout", account.AccessToken); var stderr = Pump(process.StandardError, id, "stderr", account.AccessToken);
                try
                {
                    yield return new(LauncherTaskState.Running, 1, "游戏进程运行中 · 请在游戏内正常退出；结束游戏会终止进程");
                    await process.WaitForExitAsync(token).ConfigureAwait(false);
                    await DrainAsync(process, stdout, stderr).ConfigureAwait(false);
                    logs.Write(id, GameLogLevel.Information, "lifecycle", "Process exited: " + process.ExitCode);
                    if (process.ExitCode != 0) throw new LauncherEngineException(new(LauncherErrorCode.GameExitedUnexpectedly, "游戏意外退出", "游戏未正常退出，请查看当前会话日志。", "ExitCode=" + process.ExitCode, true));
                    yield return new(LauncherTaskState.Idle, 1, "游戏已正常退出。");
                }
                finally
                {
                    if (!process.HasExited) { process.Kill(true); await process.WaitForExitAsync().ConfigureAwait(false); }
                    // Dispose readers only after process exit, then drain both pipes. No background writers survive this operation.
                    await DrainAsync(process, stdout, stderr).ConfigureAwait(false);
                    logs.Write(id, GameLogLevel.Information, "lifecycle", "Process ownership released; exit=" + process.ExitCode);
                }
            }
        }
        finally { LocalVersionSource.Dispose(version); }
        EngineUpdate Preparing(string message) { logs.Write(id, GameLogLevel.Information, "preparation", message); return new(LauncherTaskState.Preparing, 0, message, true); }
    }
    private static async Task DrainAsync(Process process, Task stdout, Task stderr)
    {
        try { await Task.WhenAll(stdout, stderr).WaitAsync(TimeSpan.FromSeconds(5)).ConfigureAwait(false); }
        catch (TimeoutException)
        {
            process.StandardOutput.Dispose(); process.StandardError.Dispose();
            try { await Task.WhenAll(stdout, stderr).ConfigureAwait(false); }
            catch (Exception e) when (e is IOException or ObjectDisposedException) { }
        }
    }
    private async Task Pump(StreamReader reader, Guid id, string source, string accessToken)
    {
        // Fixed chunks bound even malicious unterminated lines; no unbounded ReadLine allocation.
        var buffer = new char[2048]; var line = new System.Text.StringBuilder(); var discard = false; int count;
        while ((count = await reader.ReadAsync(buffer).ConfigureAwait(false)) > 0)
        {
            for (var i = 0; i < count; i++)
            {
                var character = buffer[i];
                if (character == '\n')
                {
                    logs.Write(id, source == "stderr" ? GameLogLevel.Warning : GameLogLevel.Information, source,
                        discard ? "[超长日志行已丢弃]" : Redact(line.ToString().TrimEnd('\r')));
                    line.Clear(); discard = false;
                }
                else if (!discard)
                {
                    if (line.Length >= 2048) { line.Clear(); discard = true; }
                    else line.Append(character);
                }
            }
        }
        if (line.Length > 0 || discard) logs.Write(id, GameLogLevel.Information, source, discard ? "[超长日志行已丢弃]" : Redact(line.ToString()));
        string Redact(string value)=>accessToken.Length>1?value.Replace(accessToken,"[已隐藏]",StringComparison.Ordinal):value;
    }
}
