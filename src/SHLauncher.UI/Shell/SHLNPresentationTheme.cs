using Avalonia.Styling;
using Semi.Avalonia;
using Ursa.Themes.Semi;

namespace SHLauncher.UI.Shell;

/// <summary>The new shell's single control/theme system. Legacy windows do not load it.</summary>
public sealed class SHLNPresentationTheme : Styles
{
    public SHLNPresentationTheme()
    {
        Add(new SemiTheme());
        Add(new UrsaSemiTheme());
    }
}
