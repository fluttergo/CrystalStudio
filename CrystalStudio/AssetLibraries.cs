using System.IO;
using System.Collections.Concurrent;
using System.Collections.Generic;

namespace CrystalStudio;

/// <summary>
/// Unified sprite library contract used by the editor, generators and overview atlas.
/// Crystal .lib, Shanda .wzl/.wzx and Wemade .wil/.wix all implement this so the rest
/// of the code never needs to know which container a library came from.
/// </summary>
public interface IAssetLibrary
{
    /// <summary>Main pixel-data file, used by the overview pass to share one reader per chunk.</summary>
    string Path { get; }
    int Count { get; }
    SpriteHeader? ReadHeader(int index);
    Sprite? Decode(int index);
    Sprite? DecodeOverview(BinaryReader reader, int index, int level, CancellationToken cancellation = default);
}

/// <summary>Detects the container for a library basename. Priority: .lib, then .wzl/.wzx, then .wil/.wix.</summary>
internal static class AssetLibraryFactory
{
    private static readonly ConcurrentDictionary<string, Dictionary<string, string>> DirectoryCache = new();

    internal static IAssetLibrary? TryCreate(string basePathWithoutExtension)
    {
        string? lib = Resolve(basePathWithoutExtension, ".lib");
        if (lib != null) return new CrystalLibrary(lib);

        string? wzl = Resolve(basePathWithoutExtension, ".wzl");
        string? wzx = Resolve(basePathWithoutExtension, ".wzx");
        if (wzl != null && wzx != null) return new WzlLibrary(wzx, wzl);

        string? wil = Resolve(basePathWithoutExtension, ".wil");
        string? wix = Resolve(basePathWithoutExtension, ".wix");
        if (wil != null && wix != null) return new WilLibrary(wix, wil);
        return null;
    }

    /// <summary>Case-insensitive sibling lookup (client folders mix "SmTiles2" and "smTiles2").</summary>
    private static string? Resolve(string basePath, string extension)
    {
        string direct = basePath + extension;
        if (File.Exists(direct)) return direct;
        string directory = Path.GetDirectoryName(basePath) ?? "";
        if (directory.Length == 0 || !Directory.Exists(directory)) return null;
        var names = DirectoryCache.GetOrAdd(Path.GetFullPath(directory), key =>
        {
            var map = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            try { foreach (string file in Directory.EnumerateFiles(key)) map[Path.GetFileName(file)] = file; }
            catch (IOException) { }
            catch (UnauthorizedAccessException) { }
            return map;
        });
        return names.TryGetValue(Path.GetFileName(basePath) + extension, out string? found) ? found : null;
    }
}

/// <summary>
/// Maps a legacy Shanda/Mir2 client library file name (Tiles, SmTiles, Objects, ...) to the two
/// editor library-ID families: Wemade (0/1/2...) and Shanda 2012 (100/110/120...). Both IDs are
/// registered so old Type-0 maps and newer Shanda maps resolve to the same artwork.
/// </summary>
internal static class FlatClientLibraries
{
    internal static bool TryMapIds(string name, out int wemadeId, out int shandaId)
    {
        wemadeId = shandaId = -1;
        string lower = name.ToLowerInvariant();
        // Tiles (ground): Tiles.wzl is the first, Tiles2.wzl..TilesN.wzl carry the numeric suffix.
        if (lower == "tiles") { wemadeId = 0; shandaId = 100; return true; }
        if (lower.StartsWith("tiles", StringComparison.Ordinal) && int.TryParse(lower[5..], out int tileNumber) && tileNumber is >= 2 and <= 255)
        { int sh = 100 + tileNumber - 1; if (sh <= 255) shandaId = sh; return true; }

        // SmTiles (small objects/animations): SmTiles.wzl is the first.
        if (lower == "smtiles") { wemadeId = 1; shandaId = 110; return true; }
        if (lower.StartsWith("smtiles", StringComparison.Ordinal) && int.TryParse(lower[7..], out int smNumber) && smNumber is >= 2 and <= 255)
        { int sh = 110 + smNumber - 1; if (sh <= 255) shandaId = sh; return true; }

        // Objects: Objects.wzl is the first (logical index 1); Objects2.wzl..ObjectsN.wzl carry the
        // suffix, so the series runs from index 1 (Objects.wzl) upward. The external client numbering
        // therefore starts at Objects.wzl and we support it up to 255.
        if (lower == "objects") { wemadeId = 2; shandaId = 120; return true; }
        if (lower.StartsWith("objects", StringComparison.Ordinal) && int.TryParse(lower[7..], out int objectNumber) && objectNumber is >= 2 and <= 255)
        {
            if (objectNumber + 1 <= 255) wemadeId = objectNumber + 1;
            int sh = 120 + objectNumber - 1;
            if (sh <= 255) shandaId = sh;
            return true;
        }

        // CrystalStudio's original table uses 190 for AniTiles1 (the reference editor uses 199).
        if (lower == "anitiles1") { shandaId = 190; return true; }
        return false;
    }
}
