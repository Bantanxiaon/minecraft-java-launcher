using SHLauncher.Contracts.Engine;
using SHLauncher.UI.Presentation;

namespace SHLauncher.UI.ViewModels;

public sealed class InstanceCardViewModel(InstanceInfo instance, LaunchViewModel launch, Action? selected = null) : ObservableModel
{
    public InstanceInfo Instance { get; } = instance;
    public InstanceVisualTheme VisualTheme => InstanceVisualTheme.For(Instance.Artwork);
    public string PlayedCaption => Instance.LastPlayed;
    public string SelectionCaption => IsSelected ? "当前世界" : "";
    public string AccessibleDescription => $"{Version}，{LastPlayed}，{(IsSelected ? "已选择" : "可选择")}";
    public string Name => Instance.Name;
    public string Version => $"{Instance.GameVersion} · {Instance.Loader}";
    public string Detail => Instance.Problem?.Message ?? $"Java {Instance.JavaMajor}   /   {Instance.MemoryMb / 1024} GB";
    public string LastPlayed => $"上次游玩 · {Instance.LastPlayed}";
    public string Initial => Instance.Name[..1].ToUpperInvariant();
    public bool IsSelected => launch.Selected?.Id == Instance.Id;
    public string SelectionLabel => IsSelected ? "当前实例" : "选择实例";
    public DelegateCommand SelectCommand { get; } = new(() => { if(launch.Select(instance.Id))selected?.Invoke(); }, () => launch.CanSelect);
    public void Refresh() { Raise(nameof(IsSelected)); Raise(nameof(SelectionCaption)); Raise(nameof(AccessibleDescription)); Raise(nameof(SelectionLabel)); SelectCommand.Refresh(); }
}
