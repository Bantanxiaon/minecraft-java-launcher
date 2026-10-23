using System.Security.Cryptography;
using System.Text.Json;
using CmlLib.Core;
using CmlLib.Core.Files;
using CmlLib.Core.FileExtractors;
using CmlLib.Core.Rules;
using CmlLib.Core.Version;
using SHLauncher.Contracts.Engine;
namespace SHLauncher.Engine.CmlLib;
// Consumes mature core file plans. Never invokes an installer, updater or HTTP operation.
internal static class CmlLibFileVerifier
{
    internal static async Task<IReadOnlyList<GameFile>> PlanAsync(MinecraftPath path, IVersion version, CancellationToken token)
    {
        var context = new RulesEvaluatorContext(LauncherOSRule.Current);
        var result = new List<GameFile>();
        IFileExtractor[] extractors = [new ClientFileExtractor(), new LibraryFileExtractor(JsonVersionParserOptions.ClientSide, new RulesEvaluator()), new LogFileExtractor()];
        foreach (var item in version.EnumerateToParent())
            foreach (var extractor in extractors)
                result.AddRange(await extractor.Extract(path, item, context, token).ConfigureAwait(false));
        var asset = version.GetInheritedProperty(v => v.AssetIndex) ?? throw Error("缺少资产索引信息。");
        var file = new GameFile("Asset index") { Path = path.GetIndexFilePath(asset.Id!), Hash = asset.Sha1, Size = asset.Size, Url = asset.Url };
        await CheckAsync(path, file, token).ConfigureAwait(false); result.Add(file);
        using var stream = File.OpenRead(file.Path!);
        using var index = new JsonAssetIndex(asset.Id!, await JsonDocument.ParseAsync(stream, cancellationToken: token).ConfigureAwait(false));
        if (index.IsVirtual || index.MapToResources) throw new LauncherEngineException(new(LauncherErrorCode.UnsupportedVersion, "暂不支持此版本", "当前阶段只验证现代原版资产布局。", null, false));
        result.AddRange(AssetFileExtractor.Extractor.ExtractTasksFromAssetIndex(index, path, "https://resources.download.minecraft.net/", false));
        // The URL is inert metadata; no GameFile.UpdateTask is executed.
        return result.DistinctBy(f => f.Path).ToArray();
    }
    internal static async Task CheckAsync(MinecraftPath path, GameFile file, CancellationToken token)
    {
        var target = PathSafety.EnsureContained(Path.GetDirectoryName(path.Versions)!, file.Path ?? throw Error("文件路径无效。"));
        if (!File.Exists(target)) throw Error("缺少游戏文件：" + file.Name);
        if (file.Size > 0 && new FileInfo(target).Length != file.Size) throw Error("游戏文件大小不符：" + file.Name);
        if (string.IsNullOrWhiteSpace(file.Hash)) throw Error("缺少文件校验信息：" + file.Name);
        using var stream = File.OpenRead(target);
        var hash = Convert.ToHexString(await SHA1.HashDataAsync(stream, token).ConfigureAwait(false));
        if (!hash.Equals(file.Hash, StringComparison.OrdinalIgnoreCase)) throw Error("游戏文件校验失败：" + file.Name);
    }
    internal static LauncherEngineException Error(string message) => new(new(LauncherErrorCode.GameFileMissing, "游戏文件不完整", message + " 本阶段不会自动下载或修复。", null, false));
}
