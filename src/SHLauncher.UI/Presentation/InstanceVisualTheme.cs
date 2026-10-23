using Avalonia;
using Avalonia.Media;

namespace SHLauncher.UI.Presentation;

public sealed record InstanceVisualTheme(IBrush Background, IBrush Overlay, IBrush Far, IBrush Mid, IBrush Front, IBrush Light, string Caption)
{
    private static readonly InstanceVisualTheme Forest = Create("#263F38", "#69816C", "#486B5B", "#63806A", "#354F45", "#E8DAB4", "山野之间 / ELEMENTS");
    private static readonly InstanceVisualTheme Sky = Create("#334D56", "#B9C9BD", "#879F97", "#667F75", "#3A5B53", "#F5E4BC", "从容生长 / SURVIVAL");
    private static readonly InstanceVisualTheme Slate = Create("#1C4248", "#73968E", "#467C7B", "#548E83", "#2D5E5E", "#D8E2BE", "让想法转动 / CREATE");

    public static InstanceVisualTheme For(string? artwork) => artwork switch { "sand" => Sky, "slate" => Slate, _ => Forest };
    private static InstanceVisualTheme Create(string top, string bottom, string far, string mid, string front, string light, string caption)
    {
        var color = Color.Parse(top);
        return new(
            new LinearGradientBrush { StartPoint = new RelativePoint(0, 0, RelativeUnit.Relative), EndPoint = new RelativePoint(1, 1, RelativeUnit.Relative), GradientStops = new() { new(Color.Parse(top), 0), new(Color.Parse(bottom), 1) } },
            new LinearGradientBrush { StartPoint = new RelativePoint(0, 0, RelativeUnit.Relative), EndPoint = new RelativePoint(1, 0, RelativeUnit.Relative), GradientStops = new() { new(color, 0), new(Color.FromArgb(240, color.R, color.G, color.B), .35), new(Color.FromArgb(0, color.R, color.G, color.B), 1) } },
            Brush.Parse(far), Brush.Parse(mid), Brush.Parse(front), Brush.Parse(light), caption);
    }
}
