using System.Security.Cryptography;
using System.Text.Json;
using SHLauncher.Contracts.Content;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Engine;
namespace SHLauncher.Infrastructure.Content;
public sealed class OnlineModService(IDownloadManager downloads,IInstanceModService local,Func<InstanceInfo,SemaphoreSlim> access,Func<bool> idle,
    Func<OnlineModFile,InstanceCompatibilityContext,IReadOnlyList<ResolvedContentVersion>,CancellationToken,Task<(ContentInstallPlan Plan,IReadOnlyList<OnlineModFile> Files)>> resolve):IOnlineModService
{
    private sealed record Installed(ResolvedContentVersion Version,string FileName,string Sha1);
    private sealed record RecoveryTarget(string? PreviousFile,string NewFile,string Sha1);
    private sealed record Recovery(Guid Id,IReadOnlyList<RecoveryTarget> Targets,bool HadIndex,string IndexSha1);
    private readonly Dictionary<Guid,ModInstallPreview> _previews=new();private readonly object _previewGate=new();
    private static string Game(InstanceInfo instance)=>instance.Location?.GameDirectory??throw new IOException("实例没有游戏目录。");
    private static string Safe(string root,string name)
    {
        if(name!=Path.GetFileName(name)||name.IndexOfAny(Path.GetInvalidFileNameChars())>=0||string.IsNullOrWhiteSpace(name))throw new InvalidDataException("Mod 路径无效。");
        var path=Path.GetFullPath(Path.Combine(root,name));
        for(var current=path;!string.IsNullOrEmpty(current);current=Path.GetDirectoryName(current))if((Directory.Exists(current)||File.Exists(current))&&(File.GetAttributes(current)&FileAttributes.ReparsePoint)!=0)throw new IOException("Mod 更新不能操作链接路径。");
        return path;
    }
    private static async Task<string> Hash(string path,CancellationToken token){await using var stream=File.OpenRead(path);return Convert.ToHexString(await SHA1.HashDataAsync(stream,token));}
    private static string Index(string game)=>Safe(game,".shln-mod-index.json");
    private static string Marker(string game)=>Safe(game,".shln-mod-transaction.json");
    private static async Task<List<Installed>> Load(string game,CancellationToken token)
    {
        var path=Index(game);if(!File.Exists(path))return [];
        if(new FileInfo(path).Length>4*1024*1024)throw new InvalidDataException("Mod 安装记录超过上限，原文件已保留。");
        var result=JsonSerializer.Deserialize<List<Installed>>(await File.ReadAllBytesAsync(path,token))??throw new InvalidDataException("Mod 安装记录无法读取。");
        if(result.Count>10000||result.GroupBy(v=>v.Version.Project).Any(g=>g.Count()>1))throw new InvalidDataException("Mod 安装记录存在重复或超限。");
        foreach(var item in result)Safe(Safe(game,"mods"),item.FileName);return result;
    }
    private static async Task<string> Snapshot(string game,CancellationToken token)
    {
        var mods=Safe(game,"mods");var rows=new List<string>();
        if(Directory.Exists(mods))foreach(var path in Directory.EnumerateFiles(mods).Order(StringComparer.OrdinalIgnoreCase))
        {
            token.ThrowIfCancellationRequested();var name=Path.GetFileName(path);if(!name.EndsWith(".jar",StringComparison.OrdinalIgnoreCase)&&!name.EndsWith(".jar.disabled",StringComparison.OrdinalIgnoreCase))continue;
            Safe(mods,name);rows.Add(name+":"+await Hash(path,token));if(rows.Count>10000)throw new IOException("Mod 文件数量超过上限。");
        }
        if(File.Exists(Index(game)))rows.Add("index:"+await Hash(Index(game),token));
        return Convert.ToHexString(SHA256.HashData(System.Text.Encoding.UTF8.GetBytes(string.Join("\n",rows))));
    }
    public async Task RecoverAsync(InstanceInfo instance,CancellationToken token)
    {
        if(!local.CanManage(instance))throw new IOException("仅能修改托管的独立实例。");
        var gate=access(instance);await gate.WaitAsync(token);
        try{if(!idle()||!local.CanManage(instance))throw new IOException("请先结束游戏并确认实例仍可管理。");await Recover(Game(instance),token);}
        finally{gate.Release();}
    }
    public async Task<ModInstallPreview> PreviewAsync(InstanceInfo instance,OnlineModFile selected,CancellationToken token)
    {
        if(!local.CanManage(instance))throw new IOException("仅能修改托管的独立实例。");
        var game=Game(instance);
        await RecoverAsync(instance,token);
        var before=await Snapshot(game,token);var installed=await Load(game,token);var problems=new List<string>();var valid=new List<Installed>();var mods=Safe(game,"mods");
        foreach(var item in installed)
        {
            var path=Safe(mods,item.FileName);if(!File.Exists(path))continue;
            if(!(await Hash(path,token)).Equals(item.Sha1,StringComparison.OrdinalIgnoreCase)){problems.Add(item.FileName+" 已被外部修改，不能自动替换。");continue;}valid.Add(item);
        }
        var graph=await resolve(selected,new(instance.GameVersion,instance.Loader,null),valid.Select(v=>v.Version).ToArray(),token);
        var targets=new List<ModInstallTarget>();
        foreach(var file in graph.Files)
        {
            var previous=valid.SingleOrDefault(v=>v.Version.Project==file.Version.Project);var destination=Safe(mods,file.FileName);
            if(installed.Any(i=>i.Version.Project==file.Version.Project&&File.Exists(Safe(mods,i.FileName)+".disabled")))problems.Add(file.FileName+" 的已安装版本处于禁用状态，请先启用或移除再更新。");
            var same=previous?.Version.VersionId==file.Version.VersionId&&previous.Sha1.Equals(file.Sha1,StringComparison.OrdinalIgnoreCase);
            if(File.Exists(destination)&&previous?.FileName!=file.FileName)problems.Add(file.FileName+" 与现有文件重名，未覆盖。");
            if(File.Exists(destination+".disabled"))problems.Add(file.FileName+" 存在禁用副本，请先处理。");
            targets.Add(new(file,destination,previous?.FileName,previous?.Sha1,same));
        }
        if(targets.GroupBy(t=>t.Destination,StringComparer.OrdinalIgnoreCase).Any(g=>g.Count()>1))problems.Add("计划中有重复目标文件名。");
        if(before!=await Snapshot(game,token))problems.Add("解析期间 Mod 目录发生变化，请重新预览。");
        var preview=new ModInstallPreview(Guid.NewGuid(),instance.Id,graph.Plan,targets,problems,before);
        lock(_previewGate){if(_previews.Count>=32)_previews.Clear();_previews[preview.Id]=preview;}return preview;
    }
    public async Task InstallAsync(InstanceInfo instance,ModInstallPreview preview,IProgress<DownloadTaskSnapshot>? progress,CancellationToken token)
    {
        lock(_previewGate){if(!_previews.Remove(preview.Id,out var original)||!ReferenceEquals(original,preview))throw new IOException("安装预览已过期，请重新检查。");}
        if(!preview.CanInstall||instance.Id!=preview.InstanceId||!local.CanManage(instance))throw new IOException("安装计划不可执行。");
        var game=Game(instance);var mods=Safe(game,"mods");var transaction=Safe(Safe(game,".shln-mod-transactions"),preview.Id.ToString("N"));var staged=Safe(transaction,"new");var backup=Safe(transaction,"backup");
        Directory.CreateDirectory(staged);Directory.CreateDirectory(backup);
        var pending=preview.Targets.Where(t=>!t.AlreadyInstalled).ToArray();
        var result=await downloads.DownloadAsync(new(preview.Id,"安装 Mod",staged,pending.Select(t=>new DownloadFile(Safe(staged,t.Artifact.FileName),t.Artifact.Size,t.Artifact.Sha1,null,[new(t.Artifact.Version.Project.ProviderId,t.Artifact.DownloadUri)])).ToArray()),progress,token);
        if(!result.Succeeded)throw new IOException(result.Cancelled?"Mod 下载已取消；现有文件保留。":result.Failures.FirstOrDefault()?.Message??"Mod 下载失败；现有文件保留。");
        foreach(var target in pending){var parsed=ModMetadataReader.Read(Safe(staged,target.Artifact.FileName));if(parsed.Problem is not null)throw new InvalidDataException(parsed.Problem);}
        var gate=access(instance);await gate.WaitAsync(token);
        try
        {
            if(!idle()||!local.CanManage(instance))throw new IOException("实例正在使用，尚未替换现有文件。");
            var existing=await local.ScanAsync(instance,true,token);var replaced=pending.Select(t=>t.PreviousFile).OfType<string>().ToHashSet(StringComparer.OrdinalIgnoreCase);
            var ids=existing.Where(m=>!replaced.Contains(m.FileName)).SelectMany(m=>m.Id.Split(',',StringSplitOptions.RemoveEmptyEntries)).ToHashSet(StringComparer.OrdinalIgnoreCase);
            foreach(var target in pending)
            {
                var path=Safe(staged,target.Artifact.FileName);
                if(!(await Hash(path,token)).Equals(target.Artifact.Sha1,StringComparison.OrdinalIgnoreCase))throw new IOException("已下载文件发生变化，未安装。");
                var parsed=ModMetadataReader.Read(path);
                foreach(var modId in parsed.Id.Split(',',StringSplitOptions.RemoveEmptyEntries))if(!ids.Add(modId))throw new IOException("发现重复 Mod ID："+modId+"。现有文件未被替换。");
            }
            if(File.Exists(Marker(game))||await Snapshot(game,token)!=preview.Snapshot)throw new IOException("实例内容已变化，请重新预览；现有文件保留。");
            Directory.CreateDirectory(mods);var oldIndex=await Load(game,token);var replacements=preview.Targets.Select(t=>t.Artifact.Version.Project).ToHashSet();
            var index=oldIndex.Where(i=>!replacements.Contains(i.Version.Project)).Concat(preview.Targets.Select(t=>new Installed(t.Artifact.Version,t.AlreadyInstalled?t.PreviousFile!:t.Artifact.FileName,t.Artifact.Sha1))).ToArray();
            var preparedIndex=Safe(staged,"index.json");await File.WriteAllBytesAsync(preparedIndex,JsonSerializer.SerializeToUtf8Bytes(index),token);
            var moved=new List<(string Original,string Backup)>();var published=new List<(string Destination,string Stage)>();var indexPublished=false;
            var journal=new Recovery(preview.Id,pending.Select(t=>new RecoveryTarget(t.PreviousFile,t.Artifact.FileName,t.Artifact.Sha1)).ToArray(),File.Exists(Index(game)),await Hash(preparedIndex,token));
            var preparedJournal=Safe(transaction,"journal.pending");await File.WriteAllBytesAsync(preparedJournal,JsonSerializer.SerializeToUtf8Bytes(journal),token);
            token.ThrowIfCancellationRequested();File.Move(preparedJournal,Marker(game));
            // Once publication begins it is synchronous and rollback-protected. Cancellation is honored before this boundary.
            try
            {
                foreach(var target in pending)
                {
                    if(target.PreviousFile is {} old){var source=Safe(mods,old);var saved=Safe(backup,old);File.Move(source,saved);moved.Add((source,saved));}
                    var sourceNew=Safe(staged,target.Artifact.FileName);var destination=Safe(mods,target.Artifact.FileName);File.Move(sourceNew,destination);published.Add((destination,sourceNew));
                }
                if(File.Exists(Index(game))){var saved=Safe(backup,"index.json");File.Move(Index(game),saved);moved.Add((Index(game),saved));}
                File.Move(preparedIndex,Index(game));indexPublished=true;
                File.Move(Marker(game),Safe(transaction,"committed.json"));
            }
            catch
            {
                if(indexPublished)File.Move(Index(game),preparedIndex);
                foreach(var item in published.AsEnumerable().Reverse())File.Move(item.Destination,item.Stage);
                foreach(var item in moved.AsEnumerable().Reverse())File.Move(item.Backup,item.Original);
                File.Move(Marker(game),Safe(transaction,"rolled-back.json"));throw;
            }
        }
        finally{gate.Release();}
    }
    private static async Task Recover(string game,CancellationToken token)
    {
        var marker=Marker(game);if(!File.Exists(marker))return;
        if(new FileInfo(marker).Length>256*1024)throw new InvalidDataException("Mod 恢复记录无效，文件已保留。");
        var journal=JsonSerializer.Deserialize<Recovery>(await File.ReadAllBytesAsync(marker,token))??throw new InvalidDataException("Mod 恢复记录无法读取。");
        if(journal.Id==Guid.Empty||journal.Targets.Count>128)throw new InvalidDataException("Mod 恢复记录无效。");
        var folder=Safe(Safe(game,".shln-mod-transactions"),journal.Id.ToString("N"));var staged=Safe(folder,"new");var backup=Safe(folder,"backup");var mods=Safe(game,"mods");
        // Validate every move before modifying any path. External edits stop recovery without overwriting them.
        var moves=new List<(string Source,string Destination)>();
        foreach(var target in journal.Targets.Reverse())
        {
            var destination=Safe(mods,target.NewFile);var stage=Safe(staged,target.NewFile);
            if(!File.Exists(stage)&&File.Exists(destination))
            {
                if(!(await Hash(destination,token)).Equals(target.Sha1,StringComparison.OrdinalIgnoreCase))throw new IOException("更新后的 Mod 被外部修改，自动恢复已停止，备份保留。");
                moves.Add((destination,stage));
            }
            if(target.PreviousFile is {} old)
            {
                var saved=Safe(backup,old);var original=Safe(mods,old);
                if(File.Exists(saved)){if(File.Exists(original)&&!moves.Any(m=>m.Source==original))throw new IOException("恢复目标被占用，未覆盖文件。");moves.Add((saved,original));}
            }
        }
        var savedIndex=Safe(backup,"index.json");var preparedIndex=Safe(staged,"index.json");
        if(File.Exists(Index(game))&&(File.Exists(savedIndex)||!journal.HadIndex))
        {
            if(!(await Hash(Index(game),token)).Equals(journal.IndexSha1,StringComparison.OrdinalIgnoreCase)||File.Exists(preparedIndex))throw new IOException("Mod 安装记录已变化，自动恢复停止。");
            moves.Add((Index(game),preparedIndex));
        }
        if(File.Exists(savedIndex))moves.Add((savedIndex,Index(game)));
        token.ThrowIfCancellationRequested();foreach(var move in moves)File.Move(move.Source,move.Destination);
        File.Move(marker,Safe(folder,"recovered.json"));
    }

}
