using System.Text.Json;
using SHLauncher.Bootstrap;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Contracts.Install;
internal static class RealPackWorkflow
{
    internal static async Task Run(string mode,string root,string archive,string java,string report)
    {
        // Only production composition reads credentials. Never serialize it or raw exception details.
        await using var owner=new LauncherComposition(root);
        if(mode=="real-pack-auth")
        {
            try
            {
                await owner.VerifyCurseForgeAccessAsync(default);
                await File.WriteAllTextAsync(report,JsonSerializer.Serialize(new{Configured=true,Authenticated=true,Endpoint="Official Minecraft categories",Secret="NOT RECORDED"}));
                Console.WriteLine("Configured: Yes; official API authentication: Passed");
            }
            catch(InstallationPreconditionException error)
            {
                await File.WriteAllTextAsync(report,JsonSerializer.Serialize(new{Authenticated=false,Code=error.Code,Message=error.Message,Secret="NOT RECORDED"}));
                throw new InvalidOperationException("Official API authentication failed; see sanitized report.");
            }
            return;
        }
        owner.ManualJava=java;
        await owner.Installations.InitializeAsync(default);
        var preview=await owner.ModpackInspector.InspectAsync(archive,default);
        var recipe=new InstallationRecipe(Guid.NewGuid(),preview.MinecraftVersion,preview.Loader,preview.LoaderVersion,"落幕曲 1.6.7",Guid.NewGuid(),archive,preview.ArchiveSha256);
        if(!owner.Installations.Enqueue(recipe))throw new InvalidOperationException("Production queue rejected the pack task.");
        var last="";
        while(true)
        {
            var record=owner.Installations.Records.Single(r=>r.Recipe.Id==recipe.Id);
            var state=record.Snapshot;
            var line=state.Stage+" | "+state.CompletedFiles+"/"+state.TotalFiles;
            if(line!=last){Console.WriteLine(line);last=line;}
            if(state.State is DownloadTaskState.Completed or DownloadTaskState.Failed or DownloadTaskState.Cancelled)break;
            await Task.Delay(500);
        }
        await owner.Installations.CancelAsync(recipe.Id);
        var final=owner.Installations.Records.Single(r=>r.Recipe.Id==recipe.Id);
        await File.WriteAllTextAsync(report,owner.RedactProviderSecrets(JsonSerializer.Serialize(new{Record=final,Events=owner.DownloadEvents(recipe.Id),Logs=owner.GameLogs.Snapshot(recipe.Id),Note="Real production pack install only. Menu/exit acceptance is separate."},new JsonSerializerOptions{WriteIndented=true})));
        Console.WriteLine("Installation result: "+final.Snapshot.State);
        if(final.Result is null)throw new InvalidOperationException("Real pack installation did not complete; inspect sanitized task evidence.");
    }
}
