using System.IO;
using System.Runtime.CompilerServices;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;
using System.Text.Json.Serialization.Metadata;

namespace CrystalStudio;

// Share the existing JSON options while resolving contracts without runtime code generation.
internal static class StudioJson
{
    private static readonly ConditionalWeakTable<JsonSerializerOptions, StudioJsonContext> contexts = new();

    private static JsonTypeInfo<T> Contract<T>(JsonSerializerOptions? options)
    {
        var context = options == null ? StudioJsonContext.Default
            : options.TypeInfoResolver as StudioJsonContext
                ?? contexts.GetValue(options, value => new StudioJsonContext(new JsonSerializerOptions(value)));
        return context.GetTypeInfo(typeof(T)) as JsonTypeInfo<T>
            ?? throw new NotSupportedException("Missing JSON contract: " + typeof(T).FullName);
    }

    public static T? Deserialize<T>(Stream stream, JsonSerializerOptions? options = null) =>
        JsonSerializer.Deserialize(stream, Contract<T>(options));
    public static T? Deserialize<T>(string json, JsonSerializerOptions? options = null) =>
        JsonSerializer.Deserialize(json, Contract<T>(options));
    public static T? Deserialize<T>(byte[] json, JsonSerializerOptions? options = null) =>
        JsonSerializer.Deserialize(json, Contract<T>(options));
    public static T? Deserialize<T>(ref Utf8JsonReader reader, JsonSerializerOptions? options = null) =>
        JsonSerializer.Deserialize(ref reader, Contract<T>(options));
    public static void Serialize<T>(Stream stream, T value, JsonSerializerOptions? options = null) =>
        JsonSerializer.Serialize(stream, value, Contract<T>(options));
    public static void Serialize<T>(Utf8JsonWriter writer, T value, JsonSerializerOptions? options = null) =>
        JsonSerializer.Serialize(writer, value, Contract<T>(options));
    public static byte[] SerializeToUtf8Bytes<T>(T value, JsonSerializerOptions? options = null) =>
        JsonSerializer.SerializeToUtf8Bytes(value, Contract<T>(options));
    public static JsonNode? SerializeToNode<T>(T value, JsonSerializerOptions? options = null) =>
        JsonSerializer.SerializeToNode(value, Contract<T>(options));
}

[JsonSourceGenerationOptions(GenerationMode = JsonSourceGenerationMode.Metadata)]
[JsonSerializable(typeof(LibrarySettings))]
[JsonSerializable(typeof(SetImageBindingFile))]
[JsonSerializable(typeof(List<SetBindingSegment>))]
[JsonSerializable(typeof(Dictionary<string, int[]>))]
[JsonSerializable(typeof(AttachedAssetManifest))]
[JsonSerializable(typeof(AiSettings))]
[JsonSerializable(typeof(AiEndpointSettings))]
[JsonSerializable(typeof(MapCreationProfile))]
[JsonSerializable(typeof(MapCreationPresetPackage))]
[JsonSerializable(typeof(MapCreationRecipe))]
[JsonSerializable(typeof(MapRecoveryEntry))]
[JsonSerializable(typeof(MapRecoveryData))]
[JsonSerializable(typeof(Dictionary<string, LocalText.Translation>))]
[JsonSerializable(typeof(ThemeService.Preference))]
[JsonSerializable(typeof(AssetHistoryStore.SavedAssets))]
[JsonSerializable(typeof(List<string>))]
[JsonSerializable(typeof(List<DockWorkspace.LayoutEntry>))]
[JsonSerializable(typeof(IEnumerable<DockWorkspace.LayoutEntry>))]
[JsonSerializable(typeof(StampCategoryIndex))]
[JsonSerializable(typeof(CategorizedStamp))]
[JsonSerializable(typeof(TownCatalogue))]
[JsonSerializable(typeof(TerrainCatalogue))]
[JsonSerializable(typeof(TerrainWaterCatalogue))]
[JsonSerializable(typeof(RiverArtworkCatalogue))]
[JsonSerializable(typeof(WallCatalogue))]
[JsonSerializable(typeof(TreeListDocument))]
[JsonSerializable(typeof(TreeBrushList.EntryIdentity))]
[JsonSerializable(typeof(SpecialBrushListDocument))]
[JsonSerializable(typeof(StampMetadata.Payload))]
[JsonSerializable(typeof(Front))]
[JsonSerializable(typeof(ExtractionOccurrence))]
[JsonSerializable(typeof(ExtractionReport))]
[JsonSerializable(typeof(JsonObject))]
internal partial class StudioJsonContext : JsonSerializerContext;
