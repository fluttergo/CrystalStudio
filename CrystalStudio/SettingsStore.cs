using System.IO;
using System.Text.Json;
namespace CrystalStudio;

public enum AssetSourceFormat { Default, Sanxiu, Shanda }

public sealed record LibrarySettings(string? LibraryFolder = null, string? Language = null,
    AssetSourceFormat AssetFormat = AssetSourceFormat.Default, AiSettings? Ai = null,
    bool ZeroBasedSetNumbers = false, bool PreserveOriginalMapFormat = true);

public sealed class SettingsStore
{
    public string FilePath { get; }
    public SettingsStore(string? filePath = null)
    {
        FilePath = filePath ?? Path.Combine(EditorPaths.ProfileDirectory, "settings.json");
    }
    public LibrarySettings Load()
    {
        if (File.Exists(FilePath)) return Normalize(Read(FilePath));
        return new();
    }
    private static LibrarySettings Read(string path)
    {
        using var stream = File.OpenRead(path);
        return StudioJson.Deserialize<LibrarySettings>(stream)
            ?? throw new InvalidDataException("Settings are empty. Open Settings to configure your library folder.");
    }
    private static LibrarySettings Normalize(LibrarySettings settings) => string.IsNullOrWhiteSpace(settings.LibraryFolder)
        ? settings : settings with { LibraryFolder = DataPaths.GetClientRoot(settings.LibraryFolder) };

    public void Save(LibrarySettings settings) => AtomicFile.WriteJson(FilePath, Normalize(settings));
}
