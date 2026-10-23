using CmlLib.Core;
using CmlLib.Core.Natives;
using CmlLib.Core.Rules;
using CmlLib.Core.Version;
using ICSharpCode.SharpZipLib.Zip;
using SHLauncher.Contracts.Engine;
namespace SHLauncher.Engine.CmlLib;
internal static class CmlLibNativePreparation
{
    internal static string Prepare(MinecraftPath path, IVersion version, CancellationToken token)
    {
        var context = new RulesEvaluatorContext(LauncherOSRule.Current); var evaluator = new RulesEvaluator();
        var destination = path.GetNativePath(version.Id);
        // Inspect the same local-header parser the core uses, before its extraction can write anything.
        foreach (var library in version.ConcatInheritedCollection(v => v.Libraries)
            .Where(l => l.IsClientRequired && (l.Rules is null || evaluator.Match(l.Rules, context))))
        {
            token.ThrowIfCancellationRequested();
            var relative = library.GetNativeLibraryPath(context.OS);
            if (string.IsNullOrEmpty(relative)) continue;
            var source = PathSafety.EnsureContained(Path.GetDirectoryName(path.Versions)!, Path.Combine(path.Library, relative));
            using var stream = new ZipInputStream(File.OpenRead(source)); ZipEntry? entry; long size = 0;
            while ((entry = stream.GetNextEntry()) is not null)
            {
                token.ThrowIfCancellationRequested();
                PathSafety.EnsureContained(destination, Path.Combine(destination, entry.Name));
                size += Math.Max(0, entry.Size);
                if (size > 512 * 1024 * 1024) throw new InvalidDataException("Native archive exceeds its extraction budget.");
            }
        }
        token.ThrowIfCancellationRequested();
        // Fresh destination, no Clean/Delete; synchronous core extraction is run on a worker and awaited even on cancellation.
        var result = new NativeLibraryExtractor(evaluator).Extract(path, version, context);
        token.ThrowIfCancellationRequested();
        return result;
    }
}
