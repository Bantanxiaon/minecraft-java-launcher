using System.Text.Json;
using SHLauncher.Contracts.Instances;
namespace SHLauncher.Bootstrap;
public sealed partial class LauncherComposition
{
    public async Task<string> RenameInstanceAsync(Guid id,string displayName,CancellationToken token)
    {
        if(id==Guid.Empty||string.IsNullOrWhiteSpace(displayName)||displayName.Length>64) return "请输入 1 到 64 个字符的显示名称。";
        if(!Coordinator.Snapshot.CanSelect)return "请先结束当前游戏，再修改名称。";
        var folder=Path.Combine(_workspace,"instances",id.ToString("N"));var file=Path.Combine(folder,"instance.json");
        var gate=_instanceAccess.For(folder);await gate.WaitAsync(token).ConfigureAwait(false);
        try
        {
            var record=JsonSerializer.Deserialize<ManagedInstance>(await File.ReadAllTextAsync(file,token).ConfigureAwait(false));
            if(record is null||record.Id!=id||record.SchemaVersion!=1)throw new InvalidDataException("Instance descriptor identity mismatch.");
            var part=file+"."+Guid.NewGuid().ToString("N")+".part";
            await using(var output=new FileStream(part,FileMode.CreateNew,FileAccess.Write,FileShare.None,4096,true))
            {await JsonSerializer.SerializeAsync(output,record with {DisplayName=displayName.Trim()},cancellationToken:token).ConfigureAwait(false);await output.FlushAsync(token).ConfigureAwait(false);output.Flush(true);}
            token.ThrowIfCancellationRequested();File.Move(part,file,true);
        }
        finally{gate.Release();}
        if(await Coordinator.ChangeEngineAsync(new ManagedLibraryEngine(this),token).ConfigureAwait(false))
        {IsMock=false;_managedLibrary=true;Root=Path.Combine(_workspace,"instances");Coordinator.SelectInstance(id.ToString("N"));SaveLibrarySelection();}
        return "显示名称已更新，实例目录和游戏文件没有移动。";
    }
}
