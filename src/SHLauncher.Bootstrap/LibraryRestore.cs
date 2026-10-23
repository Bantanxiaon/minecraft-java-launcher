using SHLauncher.Infrastructure.Runtime;
using SHLauncher.Contracts.Engine;
namespace SHLauncher.Bootstrap;
public sealed partial class LauncherComposition
{
    private bool _libraryPersistenceEnabled,_managedLibrary;private readonly object _librarySaveGate=new();private Task _librarySave=Task.CompletedTask;private LibraryPreference? _lastLibraryPreference;
    private void SaveLibrarySelection()
    {
        if(!_libraryPersistenceEnabled||IsMock||string.IsNullOrWhiteSpace(Root))return;
        var value=new LibraryPreference(Root,Coordinator.Selected?.Id,_managedLibrary);
        lock(_librarySaveGate)
        {
            if(value==_lastLibraryPreference)return;_lastLibraryPreference=value;
            _librarySave=_librarySave.ContinueWith(_=>
            {
                try{new LibraryPreferenceStore(_workspace).Save(value);}catch(Exception error) when(error is IOException or UnauthorizedAccessException){GameLogs.Write(Guid.Empty,GameLogLevel.Warning,"library-preference",error.GetType().Name);}
            },CancellationToken.None,TaskContinuationOptions.None,TaskScheduler.Default);
        }
    }
    public async Task InitializePresentationLibraryAsync(CancellationToken token)
    {
        _libraryPersistenceEnabled=false;LibraryPreference? saved=null;var preserveStoredPreference=false;
        try{saved=new LibraryPreferenceStore(_workspace).Read();}catch(Exception error) when(error is IOException or UnauthorizedAccessException or System.Text.Json.JsonException){preserveStoredPreference=true;GameLogs.Write(Guid.Empty,GameLogLevel.Warning,"library-preference",error.GetType().Name);}
        if(saved is {Managed:false}&&Directory.Exists(saved.Root))
        {
            await UseRootAsync(saved.Root,token);
            if(IsMock)throw new IOException("无法恢复资料库；原设置已保留。");
        }
        else
        {
            if(!await Coordinator.ChangeEngineAsync(new ManagedLibraryEngine(this),token))throw new IOException("资料库初始化时实例正在使用。");
            if(saved is {Managed:false}){preserveStoredPreference=true;GameLogs.Write(Guid.Empty,GameLogLevel.Warning,"library-preference","原资料库目录暂不可用，已保留原设置。");}
            _managedLibrary=true;Root=Path.Combine(_workspace,"instances");IsMock=false;
        }
        if(saved?.SelectedId is {} selected)Coordinator.SelectInstance(selected);
        Coordinator.Changed-=SaveLibrarySelection;Coordinator.Changed+=SaveLibrarySelection;_libraryPersistenceEnabled=!preserveStoredPreference;SaveLibrarySelection();
    }
    private async Task DrainLibraryPreferences()
    {
        Coordinator.Changed-=SaveLibrarySelection;SaveLibrarySelection();_libraryPersistenceEnabled=false;Task pending;lock(_librarySaveGate)pending=_librarySave;await pending;
    }
}
