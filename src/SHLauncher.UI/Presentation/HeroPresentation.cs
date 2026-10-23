using SHLauncher.Contracts.Engine;
using SHLauncher.UI.ViewModels;

namespace SHLauncher.UI.Presentation;

public sealed record HeroPresentation(string? InstanceId, string InstanceName, string Description, string Version, string Loader, string Runtime, InstanceVisualTheme Theme, LaunchViewModel Launch)
{
    public static HeroPresentation From(InstanceInfo? instance, LaunchViewModel launch) => new(instance?.Id,
        instance?.Name ?? "你的世界，正在这里", instance?.Description ?? "稍等片刻。",
        $"Minecraft {instance?.GameVersion ?? "—"}", instance?.Loader ?? "—",
        $"Java {instance?.JavaMajor ?? 0} · {(instance?.MemoryMb ?? 0) / 1024} GB", InstanceVisualTheme.For(instance?.Artwork), launch);
}
