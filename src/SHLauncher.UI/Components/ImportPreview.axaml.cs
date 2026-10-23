using Avalonia.Controls;
using Avalonia.Input;
namespace SHLauncher.UI.Components;
public sealed partial class ImportPreview:UserControl
{
    public ImportPreview()=>InitializeComponent();
    internal void FocusInitial()=>CancelButton.Focus(NavigationMethod.Tab);
}
