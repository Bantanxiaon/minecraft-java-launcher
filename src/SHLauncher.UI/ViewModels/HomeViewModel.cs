using SHLauncher.UI.Presentation;

namespace SHLauncher.UI.ViewModels;

public sealed class HomeViewModel : ObservableModel, IDisposable
{
    private readonly Func<string> _modeNote;
    public string ModeNote=>_modeNote();
    public HeroPresentation Hero { get; private set; }
    public LaunchViewModel Launch { get; }
    public LibraryViewModel Library { get; }
    public System.Windows.Input.ICommand? AccountCommand {get;set;}
    public AccountsViewModel? Accounts {get;set;}
    public string Metadata => Launch.Selected is { } i ? $"Minecraft {i.GameVersion} · {i.Loader}" : "请先选择或安装实例";
    public double HeroFontSize => InstanceName.Length > 24 ? 44 : 64;
    public string InstanceName => Launch.Selected?.Name ?? "选择实例";
    public string Description => Launch.Selected?.Description ?? "稍等片刻。";
    public string Version => $"Minecraft {Launch.Selected?.GameVersion ?? "—"}";
    public string Loader => Launch.Selected?.Loader ?? "—";
    public string Runtime => $"Java {Launch.Selected?.JavaMajor ?? 0} · {(Launch.Selected?.MemoryMb ?? 0) / 1024} GB";
    public HomeViewModel(LaunchViewModel launch, LibraryViewModel library,Func<string>? modeNote=null)
    { _modeNote=modeNote??(()=>"演示资料库");Launch = launch; Library = library; Hero = HeroPresentation.From(launch.Selected, launch); launch.Refreshed += Refresh; }
    private void Refresh()
    {
        if (Hero.InstanceId != Launch.Selected?.Id) { Hero = HeroPresentation.From(Launch.Selected, Launch); Raise(nameof(Hero)); }
        foreach (var name in new[] { nameof(InstanceName), nameof(HeroFontSize), nameof(Metadata), nameof(Description), nameof(Version), nameof(Loader), nameof(Runtime),nameof(ModeNote) }) Raise(name);
    }
    public void Dispose() => Launch.Refreshed -= Refresh;
}
