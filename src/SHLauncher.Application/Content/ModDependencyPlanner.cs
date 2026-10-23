using SHLauncher.Contracts.Content;
namespace SHLauncher.Application.Content;
/// <summary>Builds a bounded dependency graph before any instance files are written.</summary>
public sealed class ModDependencyPlanner(IReadOnlyList<IOnlineModProvider> providers)
{
    public async Task<(ContentInstallPlan Plan,IReadOnlyList<OnlineModFile> Files)> PlanAsync(OnlineModFile root,InstanceCompatibilityContext context,IReadOnlyList<ResolvedContentVersion> installed,CancellationToken token)
    {
        var nodes=new Dictionary<ContentIdentity,OnlineModFile>();var problems=new List<CompatibilityResult>();
        void Problem(CompatibilityCode code,string reason,string id)=>problems.Add(new(code,reason,[id]));
        async Task Visit(OnlineModFile file,int depth)
        {
            token.ThrowIfCancellationRequested();var version=file.Version;var identity=version.Project;
            if(depth>32||nodes.Count>=128){Problem(CompatibilityCode.Unknown,"依赖数量或深度超过安全上限。",identity.ProjectId);return;}
            if(nodes.TryGetValue(identity,out var previous)){if(previous.Version.VersionId!=version.VersionId)Problem(CompatibilityCode.DependencyConflict,"同一项目要求不同的精确版本。",identity.ProjectId);return;}
            if(!version.MinecraftVersions.Contains(context.MinecraftVersion)){Problem(CompatibilityCode.WrongMinecraftVersion,"Minecraft 版本不匹配。",identity.ProjectId);return;}
            if(!version.Loaders.Contains(context.Loader,StringComparer.OrdinalIgnoreCase)){Problem(CompatibilityCode.WrongLoader,"Mod Loader 不匹配。",identity.ProjectId);return;}
            nodes.Add(identity,file);
            foreach(var edge in version.Dependencies)
            {
                if(edge.Kind==DependencyKind.Unknown){Problem(CompatibilityCode.Unknown,"服务返回了无法识别的依赖关系。",identity.ProjectId);continue;}
                if(edge.Kind!=DependencyKind.Required)continue;
                if(edge.Constraint.MinimumVersion is not null||edge.Constraint.MaximumVersion is not null||edge.Constraint.UnparsedProviderConstraint is not null){Problem(CompatibilityCode.Unknown,"依赖版本范围尚不能可靠解析。",edge.Target.ProjectId);continue;}
                var provider=providers.SingleOrDefault(p=>p.Id==edge.Target.ProviderId);
                if(provider is null){Problem(CompatibilityCode.MissingRequiredDependency,"缺少对应依赖服务。",edge.Target.ProjectId);continue;}
                try
                {
                    OnlineModFile dependency;
                    if(edge.Constraint.ExactVersionId is {} exact)dependency=await provider.ExactVersionAsync(edge.Target.ProjectId,exact,token);
                    else
                    {
                        if(string.IsNullOrEmpty(edge.Target.ProjectId)){Problem(CompatibilityCode.Unknown,"依赖未提供项目或版本 ID。",identity.ProjectId);continue;}
                        var choices=await provider.VersionsAsync(edge.Target.ProjectId,context,token);
                        dependency=choices.Where(v=>v.Version.Channel==ReleaseChannel.Release).OrderByDescending(v=>v.Version.PublishedAt).FirstOrDefault()??throw new IOException("没有兼容的正式依赖版本，请先检查依赖版本。");
                    }
                    await Visit(dependency,depth+1);
                }
                catch(OperationCanceledException){throw;}
                catch(Exception error) when(error is IOException or InvalidDataException or InvalidOperationException){Problem(CompatibilityCode.MissingRequiredDependency,error.Message,edge.Target.ProjectId);}
            }

        }
        await Visit(root,0);
        var effective=installed.Where(v=>!nodes.ContainsKey(v.Project)).Concat(nodes.Values.Select(f=>f.Version)).ToArray();
        foreach(var version in effective)
        foreach(var edge in version.Dependencies.Where(e=>e.Kind==DependencyKind.Incompatible))
        {
            if(effective.Any(other=>other.Project==edge.Target&&(edge.Constraint.ExactVersionId is null||edge.Constraint.ExactVersionId==other.VersionId)))Problem(CompatibilityCode.DependencyConflict,$"{version.Project.ProjectId} 与 {edge.Target.ProjectId} 冲突。",edge.Target.ProjectId);
        }
        foreach(var version in effective)
        foreach(var edge in version.Dependencies.Where(e=>e.Kind==DependencyKind.Required&&!string.IsNullOrEmpty(e.Target.ProjectId)))
            if(!effective.Any(other=>other.Project==edge.Target&&(edge.Constraint.ExactVersionId is null||edge.Constraint.ExactVersionId==other.VersionId)))Problem(CompatibilityCode.MissingRequiredDependency,"更新后无法满足已有 Mod 的必需依赖。",edge.Target.ProjectId);
        var plan=new ContentInstallPlan(nodes.Values.Select(f=>new ContentPlanNode(f.Version,installed.Any(v=>v.Project==f.Version.Project&&v.VersionId==f.Version.VersionId),f.Version.Project==root.Version.Project?"所选 Mod":"必需依赖")).ToArray(),problems);
        return(plan,nodes.Values.ToArray());
    }
}
