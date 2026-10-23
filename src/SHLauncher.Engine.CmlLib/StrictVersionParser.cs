using CmlLib.Core.Version;
namespace SHLauncher.Engine.CmlLib;
internal static class StrictVersionParser
{
    internal static IVersion Read(Stream input)
    {
        var version = JsonVersionParser.ParseFromJsonStream(input, new JsonVersionParserOptions { SkipError = false });
        try
        {
            // Core properties are lazy; force validation before declaring a profile usable.
            _ = version.Client; _ = version.AssetIndex; _ = version.Libraries; _ = version.Logging;
            if (version is JsonVersion raw && raw.GetProperty("libraries") is { } librariesJson)
            {
                using var libraries = System.Text.Json.JsonDocument.Parse(librariesJson);
                if (libraries.RootElement.ValueKind != System.Text.Json.JsonValueKind.Array || libraries.RootElement.GetArrayLength() != version.Libraries.Count)
                    throw new InvalidDataException("Core parser rejected one or more library metadata entries.");
                foreach (var library in libraries.RootElement.EnumerateArray())
                    if (library.TryGetProperty("rules", out var rules)) ValidateRules(rules);
            }
            _ = version.GetJvmArguments(false); _ = version.GetGameArguments(false);
            if (version is JsonVersion json && json.GetProperty("arguments") is { } argumentsJson)
            {
                using var arguments = System.Text.Json.JsonDocument.Parse(argumentsJson);
                foreach (var key in new[] { "jvm", "game" })
                    if (arguments.RootElement.TryGetProperty(key, out var array))
                    {
                        var parsed = key == "jvm" ? version.GetJvmArguments(false) : version.GetGameArguments(false);
                        if (array.ValueKind != System.Text.Json.JsonValueKind.Array || array.GetArrayLength() != parsed.Count)
                            throw new InvalidDataException("Core parser rejected one or more argument entries.");
                        foreach (var entry in array.EnumerateArray())
                            if (entry.ValueKind == System.Text.Json.JsonValueKind.Object)
                            {
                                if (entry.TryGetProperty("rules", out var rules)) ValidateRules(rules);
                                else if (entry.TryGetProperty("compatibilityRules", out var compatibility)) ValidateRules(compatibility);
                                var value = entry.TryGetProperty("value", out var singular) ? singular : entry.GetProperty("values");
                                if (value.ValueKind == System.Text.Json.JsonValueKind.Array && value.EnumerateArray().Any(v => v.ValueKind != System.Text.Json.JsonValueKind.String || string.IsNullOrEmpty(v.GetString())))
                                    throw new InvalidDataException("Argument value was silently omitted by core parser.");
                            }
                    }
            }
            return version;
        }
        catch { (version as IDisposable)?.Dispose(); throw; }
    }
    private static void ValidateRules(System.Text.Json.JsonElement rules)
    {
        var expected = rules.ValueKind switch {
            System.Text.Json.JsonValueKind.Object => 1,
            System.Text.Json.JsonValueKind.Array => rules.GetArrayLength(),
            _ => throw new InvalidDataException("Invalid rule metadata shape.") };
        if (JsonRulesParser.Parse(rules).Count != expected) throw new InvalidDataException("Core parser rejected one or more rule entries.");
    }
}
