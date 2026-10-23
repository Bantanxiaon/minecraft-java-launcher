using System.Security.Cryptography;
using System.Text.Json;
using ICSharpCode.SharpZipLib.Zip;
using SHLauncher.Contracts.Install;
using SHLauncher.Contracts.Instances;
using SHLauncher.Contracts.Downloads;
namespace SHLauncher.Infrastructure.Install;
public sealed class ManagedInstanceInstallService(string dataRoot,Func<string,IInstallationBackend> createBackend,
    IDownloadManager downloads,IJsonMetadataSource metadata,IModpackInspector inspector,Func<bool> hasProviderAccess):IInstallationBackend
{
    public async Task<InstallResult> InstallAsync(InstallationRecipe recipe, IProgress<InstallProgress>? progress, CancellationToken token)
    {
        var final = ArchivePathPolicy.Resolve(dataRoot, "instances/"+recipe.InstanceKey.ToString("N"));
        var descriptor = Path.Combine(final, "instance.json");
        if(recipe.InstanceKey==Guid.Empty||string.IsNullOrWhiteSpace(recipe.DisplayName))throw new ArgumentException("Missing stable instance identity or display name.");
        var existing = File.Exists(descriptor);
        if(existing)
        {
            var previous=JsonSerializer.Deserialize<ManagedInstance>(await File.ReadAllTextAsync(descriptor,token).ConfigureAwait(false));
            if(previous is null||previous.Id!=recipe.InstanceKey||previous.MinecraftVersion!=recipe.MinecraftVersion||previous.Loader!=recipe.Loader||previous.LoaderVersion!=recipe.LoaderVersion||recipe.ArchivePath is not null)
                throw new InvalidOperationException("现有实例不接受隐式版本替换或整合包覆盖，请安装为新实例。");
        }
        SHLauncher.Contracts.Install.ModpackPreview? pack = null;
        IReadOnlyList<SHLauncher.Contracts.Install.PlannedArtifact>? packArtifacts = null;
        if (recipe.ArchivePath is not null)
        {
            progress?.Report(new("重新校验原始整合包与安装计划"));
            pack = await inspector.InspectAsync(recipe.ArchivePath,token).ConfigureAwait(false);
            if (pack.ArchiveSha256 != recipe.ArchiveSha256 || pack.MinecraftVersion != recipe.MinecraftVersion || pack.Loader != recipe.Loader || pack.LoaderVersion != recipe.LoaderVersion)
                throw new InvalidDataException("整合包自预览后发生变化，或版本计划不一致。请重新预览。");
            if (pack.RemoteFiles.Any(f=>f.Required) && !hasProviderAccess())
                throw new InstallationPreconditionException("ProviderAccessMissing","尚未配置 SH Launcher 自有 CurseForge 访问。请配置 settings/curseforge.local.json 或指定本地配置文件，重启后重试；未创建实例，也未下载整合包内容。");
            packArtifacts = await new CurseForgePackArtifactResolver(metadata,recipe.Id,(done,total)=>progress?.Report(new("解析整合包远程条目 "+done+" / "+total))).ResolveAsync(pack,token).ConfigureAwait(false);
        }
        var stageParent = existing ? Path.Combine(dataRoot, "instances") : Path.Combine(dataRoot, "temp", "install", recipe.Id.ToString("N"));
        var stage = Path.Combine(stageParent, recipe.InstanceKey.ToString("N"));
        if (!existing && Directory.Exists(final)) throw new IOException("Instance folder exists without a published descriptor.");
        if(packArtifacts is not null)
        {
            foreach(var artifact in packArtifacts) ArchivePathPolicy.Resolve(Path.Combine(stage,"game"),artifact.RelativePath);
            if(packArtifacts.Where(a=>a.ArchiveEntry is null).GroupBy(a=>a.RelativePath,StringComparer.OrdinalIgnoreCase).Any(g=>g.Count()>1))
                throw new InvalidDataException("整合包远程目标路径冲突；尚未开始实例安装。");
        }
        var backend=createBackend(stageParent);
        if(backend is not IMinecraftVersionService minecraftVersions)throw new NotSupportedException("安装后端缺少版本预检接口。");
        var releases=await minecraftVersions.GetVersionsAsync(token).ConfigureAwait(false);
        if(!releases.Any(v=>v.Id==recipe.MinecraftVersion&&v.Kind==MinecraftVersionKind.Release))throw new InvalidDataException("Minecraft 正式版不在当前官方目录中。");
        if(recipe.Loader!=LoaderType.Vanilla)
        {
            if(backend is not ILoaderVersionService loaderVersions)throw new NotSupportedException("安装后端缺少 Loader 预检接口。");
            var available=await loaderVersions.GetVersionsAsync(recipe.Loader,recipe.MinecraftVersion,token).ConfigureAwait(false);
            if(!available.Any(v=>v.Version==recipe.LoaderVersion&&v.MinecraftVersion==recipe.MinecraftVersion&&v.Loader==recipe.Loader))
                throw new InvalidDataException("整合包指定的精确 Loader 不在当前官方目录中；未替换版本。");
        }
        if(packArtifacts is not null)
        {
            var plan=new UnifiedInstallPlan(recipe.InstanceKey,recipe.DisplayName,recipe.MinecraftVersion,recipe.Loader,recipe.LoaderVersion,packArtifacts,pack!.ArchiveSha256);
            var planDirectory=Path.Combine(dataRoot,"temp","plans");Directory.CreateDirectory(planDirectory);
            var safe=plan with{Artifacts=plan.Artifacts.Select(a=>a with{Sources=a.Sources.Select(s=>s with{Uri=new Uri(s.Uri.GetLeftPart(UriPartial.Path))}).ToArray()}).ToArray()};
            var localTargets=packArtifacts.Where(a=>a.ArchiveEntry is not null).Select(a=>a.RelativePath).ToHashSet(StringComparer.OrdinalIgnoreCase);
            await File.WriteAllTextAsync(Path.Combine(planDirectory,recipe.Id.ToString("N")+".json"),JsonSerializer.Serialize(new{
                Plan=safe,OverriddenRemoteTargets=packArtifacts.Where(a=>a.ArchiveEntry is null&&localTargets.Contains(a.RelativePath)).Select(a=>new{a.ProviderId,a.ArtifactId,a.RelativePath,Reason="Replaced by explicit archive override after remote planning"})
            }),token).ConfigureAwait(false);
        }
        var result = await backend.InstallAsync(recipe, progress, token).ConfigureAwait(false);
        token.ThrowIfCancellationRequested();
        if (pack is not null && packArtifacts is not null)
        {
            var content=await ApplyPackAsync(pack,packArtifacts,stage,recipe.Id,progress,token).ConfigureAwait(false);
            result=result with {FileCount=result.FileCount+content.Files.Count+pack.LocalFiles.Count,
                NetworkBytes=result.NetworkBytes+content.Files.Sum(f=>f.NetworkBytes),ReusedFiles=result.ReusedFiles+content.Files.Count(f=>f.Reused)};
        }
        var record = new ManagedInstance(recipe.InstanceKey, recipe.DisplayName, recipe.MinecraftVersion, recipe.Loader, recipe.LoaderVersion, result.InstanceId);
        Directory.CreateDirectory(Path.Combine(stage, "game"));
        var part = Path.Combine(stage, "instance.json." + Guid.NewGuid().ToString("N") + ".part");
        await using (var output = new FileStream(part, FileMode.CreateNew, FileAccess.Write, FileShare.None, 8192, true))
        { await JsonSerializer.SerializeAsync(output, record, cancellationToken: token).ConfigureAwait(false); await output.FlushAsync(token).ConfigureAwait(false); output.Flush(true); }
        token.ThrowIfCancellationRequested(); File.Move(part, Path.Combine(stage, "instance.json"), true);
        if (!existing) { Directory.CreateDirectory(Path.GetDirectoryName(final)!); Directory.Move(stage, final); }
        return result with { Root = Path.Combine(final, "runtime") };
    }
    private async Task<DownloadResult> ApplyPackAsync(ModpackPreview preview,IReadOnlyList<PlannedArtifact> artifacts,string stage,Guid id,IProgress<InstallProgress>? progress,CancellationToken token)
    {
        var game=Path.Combine(stage,"game");
        // Destination planning is complete before any content network request. Local overrides deliberately take precedence.
        var locals=artifacts.Where(a=>a.ArchiveEntry is not null).ToArray();
        var overridden=locals.Select(a=>a.RelativePath).ToHashSet(StringComparer.OrdinalIgnoreCase);
        var remote=artifacts.Where(a=>a.ArchiveEntry is null&&!overridden.Contains(a.RelativePath)).ToArray();
        if(remote.GroupBy(a=>a.RelativePath,StringComparer.OrdinalIgnoreCase).Any(g=>g.Count()>1))throw new InvalidDataException("远程文件目标冲突。");
        var files=remote.Select(a=>new DownloadFile(ArchivePathPolicy.Resolve(game,a.RelativePath),a.Size,a.Sha1,a.Sha256,a.Sources)).ToArray();
        foreach(var item in locals)ArchivePathPolicy.Resolve(game,item.RelativePath);
        progress?.Report(new("下载整合包所需文件"));
        var transferred=await downloads.DownloadAsync(new(id,preview.DisplayName,game,files),new PackProgress(p=>progress?.Report(new("下载整合包文件",p))),token).ConfigureAwait(false);
        token.ThrowIfCancellationRequested();
        if(!transferred.Succeeded && transferred.Failures.Any(f=>f.SourceId=="CurseForge CDN" && f.Code is "HTTP401" or "HTTP403"))
            throw new InstallationPreconditionException("ProviderAccessRejected","CurseForge 文件下载被拒绝（401/403）。请检查 SH Launcher 自有访问权限及作者的分发限制；未发布实例。");
        if(!transferred.Succeeded)throw new IOException("整合包文件下载失败："+transferred.Failures.FirstOrDefault()?.Message);
        progress?.Report(new("应用整合包 overrides"));
        await using var original=new FileStream(preview.ArchivePath,FileMode.Open,FileAccess.Read,FileShare.Read,81920,true);
        var originalHash=Convert.ToHexString(await SHA256.HashDataAsync(original,token).ConfigureAwait(false));
        if(!originalHash.Equals(preview.ArchiveSha256,StringComparison.OrdinalIgnoreCase))throw new InvalidDataException("原始整合包已改变，请重新预览。");
        original.Position=0;using var zip=new ZipFile(original){IsStreamOwner=false};
        foreach(var artifact in locals)
        {
            token.ThrowIfCancellationRequested();var target=ArchivePathPolicy.Resolve(game,artifact.RelativePath);
            var entry=zip.GetEntry(artifact.ArchiveEntry!)??throw new InvalidDataException("Archive entry missing after preview.");
            Directory.CreateDirectory(Path.GetDirectoryName(target)!);
            var part=target+"."+Guid.NewGuid().ToString("N")+".part";
            using(var input=zip.GetInputStream(entry))
            await using(var output=new FileStream(part,FileMode.CreateNew,FileAccess.Write,FileShare.None,81920,true))
            { await input.CopyToAsync(output,token).ConfigureAwait(false);await output.FlushAsync(token).ConfigureAwait(false);output.Flush(true); }
            using(var copied=File.OpenRead(part))
                if(new FileInfo(part).Length!=artifact.Size || !Convert.ToHexString(await SHA256.HashDataAsync(copied,token).ConfigureAwait(false)).Equals(artifact.Sha256,StringComparison.OrdinalIgnoreCase))
                    throw new InvalidDataException("Override copy integrity failed: "+artifact.RelativePath);
            token.ThrowIfCancellationRequested();File.Move(part,target,true);
        }
        progress?.Report(new("整合包内容已验证，准备发布实例"));
        return transferred;
    }
    private sealed class PackProgress(Action<DownloadTaskSnapshot> report):IProgress<DownloadTaskSnapshot>{public void Report(DownloadTaskSnapshot value)=>report(value);}
}
