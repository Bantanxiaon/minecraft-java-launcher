using System.Windows.Input;

namespace SHLauncher.UI.Presentation;

public sealed class DelegateCommand(Action action, Func<bool>? canExecute = null) : ICommand
{
    public event EventHandler? CanExecuteChanged;
    public bool CanExecute(object? parameter) => canExecute?.Invoke() ?? true;
    public void Execute(object? parameter) { if (CanExecute(parameter)) action(); }
    public void Refresh() => CanExecuteChanged?.Invoke(this, EventArgs.Empty);
}

public sealed class AsyncCommand(Func<Task> action, Func<bool> canExecute, Action<Exception> onError) : ICommand
{
    private readonly object _gate=new();
    private readonly HashSet<Task> _pending=new();
    public event EventHandler? CanExecuteChanged;
    public bool CanExecute(object? parameter) => canExecute();
    public async void Execute(object? parameter)
    {
        if (!CanExecute(parameter)) return;
        var work=RunAsync();
        lock(_gate)_pending.Add(work);
        try{await work;}finally{lock(_gate)_pending.Remove(work);}
    }
    private async Task RunAsync()
    {
        try{await action();}
        catch(Exception error){onError(error);}
    }
    public Task DrainAsync(){lock(_gate)return Task.WhenAll(_pending.ToArray());}
    public void Refresh() => CanExecuteChanged?.Invoke(this, EventArgs.Empty);
}
