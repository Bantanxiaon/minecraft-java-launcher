using System.Collections.ObjectModel;
using SHLauncher.UI.Presentation;

namespace SHLauncher.UI.ViewModels;

public sealed class LibraryViewModel : ObservableModel, IDisposable
{
    public System.Windows.Input.ICommand? ModsCommand {get;set;}
    public LaunchViewModel Launch { get; }
    public System.Windows.Input.ICommand? ImportCommand { get; }
    public System.Windows.Input.ICommand? CreateCommand { get; }
    public string CountLabel => Cards.Count + " 个实例";
    public bool IsEmpty => Cards.Count==0;
    public ObservableCollection<InstanceCardViewModel> Cards { get; } = new();
    public string SelectionHint => Launch.CanSelect ? "选择实例后返回主页。" : "游戏任务进行中，结束后即可切换实例。";
    private readonly Action? _selected;
    public LibraryViewModel(LaunchViewModel launch, System.Windows.Input.ICommand? import = null, System.Windows.Input.ICommand? create = null, Action? selected = null) { _selected=selected; Launch = launch; ImportCommand=import;CreateCommand=create;launch.Refreshed += Refresh; }
    private void Refresh()
    {
        if (!Cards.Select(c => c.Instance).SequenceEqual(Launch.Instances))
        {
            Cards.Clear();
            foreach (var instance in Launch.Instances) Cards.Add(new(instance, Launch, _selected));
        }
        foreach (var card in Cards) card.Refresh();
        Raise(nameof(SelectionHint));Raise(nameof(CountLabel));Raise(nameof(IsEmpty));
    }
    public void Dispose() => Launch.Refreshed -= Refresh;
}
