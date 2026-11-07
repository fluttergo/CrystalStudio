using System.IO;
using System.Security.Cryptography;
using System.Text;

namespace CrystalStudio;

/// <summary>
/// Number base used for composition model names and exported PNG file names.
/// Domestic (盛大/GM 工具箱) editors label .set sections <b>0-based</b> (section [0] → mir_0),
/// while the original imported palette in this editor used 1-based names (section [0] → mir_1).
/// The .set file itself always starts at [0]; only the label changes. Default stays 1-based so
/// previously exported files keep their names; the palette exposes a toggle to switch to 0-based.
/// </summary>
public static class SetNumbering
{
    /// <summary>0 = domestic 0-based labels, 1 = legacy 1-based labels.</summary>
    public static int Base { get; set; } = 1;

    public static string Name(string prefix, int section) => $"{prefix}_{section + Base}";
}

/// <summary>
/// One row of a Shanda .set file. X/Y are <b>isometric map-cell coordinates</b> relative to
/// the composition anchor (the client draws them at X*48 / Y*32 pixels), not pixel offsets.
/// Image is a global index into the Objects*.wzl family; -1 is an empty placeholder slot.
/// Columns 5-7 are the light/animation controls: column 5 is the <b>frame count</b> of the
/// sprite's animation loop (7, 8 or 10 in the standard client) and doubles as the additive
/// blend level; column 6 is the frame delay (tick); column 7 is the literal <c>t</c>/<c>f</c>
/// additive toggle. A light/flame row with <c>t</c> is therefore an <b>animated cell</b>: the
/// client advances Image by one per tick and wraps at FrameCount, drawing each frame with
/// SourceAlpha/One compositing so its near-black palette entries add nothing instead of
/// painting an opaque black square.
/// </summary>
public readonly record struct SetPiece(int X, int Y, int Secondary, int Image, int Type,
    int FrameCount, int FrameTick, bool Additive, int AuxA, int AuxB, int AuxC)
{
    public bool HasImage => Image >= 0;
    /// <summary>True when this cell must be written as an animated additive front sprite.</summary>
    public bool Animated => Additive && FrameCount > 1;
}

/// <summary>One [section] of a .set file: a complete composite object (building, wall, stall...).</summary>
public sealed record SetComposition(int Section, string Source, IReadOnlyList<SetPiece> Pieces, string? Name = null)
{
    /// <summary>Category is the .set file name without extension, e.g. mir2.</summary>
    public string Category => SetCompositionFile.Prefix(Source);
    /// <summary>Domestic editors name imported models "&lt;prefix&gt;_&lt;section+base&gt;".</summary>
    public string DisplayName => Name ?? SetNumbering.Name(Category, Section);
    public int ImagePieces => Pieces.Count(p => p.HasImage);
    public int CellCount => Pieces.Where(p => p.HasImage).Select(p => (p.X, p.Y)).Distinct().Count();
    public int MinX => Pieces.Count == 0 ? 0 : Pieces.Min(p => p.X);
    public int MaxX => Pieces.Count == 0 ? 0 : Pieces.Max(p => p.X);
    public int MinY => Pieces.Count == 0 ? 0 : Pieces.Min(p => p.Y);
    public int MaxY => Pieces.Count == 0 ? 0 : Pieces.Max(p => p.Y);
    public int GridWidth => MaxX - MinX + 1;
    public int GridHeight => MaxY - MinY + 1;
    /// <summary>Frame count of the animated lights in this composition; 0 means fully static.
    /// Standard clients use one loop length for every light in a section.</summary>
    public int AnimationFrames => Pieces.Where(p => p.Animated).Select(p => p.FrameCount).DefaultIfEmpty(0).Max();
    /// <summary>Frame delay of the animated lights, in 100 ms ticks (0 = one frame per tick).</summary>
    public int AnimationTick => Pieces.Where(p => p.Animated).Select(p => p.FrameTick).DefaultIfEmpty(0).Max();
    public string Label => DisplayName;
    public string Detail => LocalText.Format("{0:N0} cells · {1:N0} images · {2}×{3} grid",
        CellCount, ImagePieces, GridWidth, GridHeight);
}

public sealed record SetCompositionTable(IReadOnlyList<SetComposition> Compositions,
    IReadOnlyList<string> Sources, IReadOnlyList<string> Warnings,
    IReadOnlyList<string>? Categories = null)
{
    public int TotalPieces => Compositions.Sum(c => c.Pieces.Count);
    public int ImagePieces => Compositions.Sum(c => c.ImagePieces);
    public IReadOnlyList<string> CategoryList => Categories ?? Sources.Select(SetCompositionFile.Prefix)
        .Distinct(StringComparer.OrdinalIgnoreCase).ToArray();
}

/// <summary>Streaming parser for the Shanda standard composition library (mir.set / mir2.set).</summary>
public static class SetCompositionFile
{
    /// <summary>The category prefix of a (possibly merged) source label.</summary>
    public static string Prefix(string source)
    {
        string first = source.Split(" + ", StringSplitOptions.RemoveEmptyEntries).FirstOrDefault() ?? source;
        return Path.GetFileNameWithoutExtension(first);
    }

    public static IReadOnlyList<SetComposition> Parse(string path, CancellationToken cancellation = default)
    {
        string source = Path.GetFileName(path);
        var result = new List<SetComposition>();
        int section = -1;
        var pieces = new List<SetPiece>();
        foreach (string raw in File.ReadLines(path, Encoding.Latin1))
        {
            cancellation.ThrowIfCancellationRequested();
            string line = raw.Trim();
            if (line.Length == 0) continue;
            if (line[0] == '[' && line[^1] == ']' && int.TryParse(line[1..^1], out int next))
            {
                if (section >= 0) result.Add(new(section, source, pieces.ToArray()));
                section = next; pieces = new List<SetPiece>(); continue;
            }
            if (section < 0) continue;
            string[] f = line.Split(' ', StringSplitOptions.RemoveEmptyEntries);
            if (f.Length < 11) continue;
            if (!int.TryParse(f[0], out int x) || !int.TryParse(f[1], out int y) ||
                !int.TryParse(f[2], out int secondary) || !int.TryParse(f[3], out int image) ||
                !int.TryParse(f[4], out int type)) continue;
            int.TryParse(f[8], out int auxA);
            int.TryParse(f[9], out int auxB);
            int.TryParse(f[10], out int auxC);
            int.TryParse(f[5], out int frameCount);
            int.TryParse(f[6], out int frameTick);
            bool additive = f[7].Equals("t", StringComparison.OrdinalIgnoreCase);
            pieces.Add(new(x, y, secondary, image, type, frameCount, frameTick, additive, auxA, auxB, auxC));
        }
        if (section >= 0) result.Add(new(section, source, pieces.ToArray()));
        return result;
    }

    /// <summary>Load every *.set in a client data folder and merge identical compositions.</summary>
    public static SetCompositionTable Load(string folder, CancellationToken cancellation = default)
    {
        var sources = new List<string>();
        var warnings = new List<string>();
        var byFingerprint = new Dictionary<string, SetComposition>();
        foreach (string path in Directory.EnumerateFiles(folder, "*.set", new EnumerationOptions
                 { MatchCasing = MatchCasing.CaseInsensitive, IgnoreInaccessible = true }).OrderBy(p => p, StringComparer.OrdinalIgnoreCase))
        {
            cancellation.ThrowIfCancellationRequested();
            try
            {
                string name = Path.GetFileName(path);
                sources.Add(name);
                foreach (var composition in Parse(path, cancellation))
                {
                    // A section can appear in both files with identical content; keep one entry.
                    var hash = new StringBuilder();
                    hash.Append(composition.Section);
                    foreach (var p in composition.Pieces)
                        hash.Append('|').Append(p.X).Append(',').Append(p.Y).Append(',').Append(p.Secondary)
                            .Append(',').Append(p.Image).Append(',').Append(p.Type)
                            .Append(',').Append(p.Additive ? 't' : 'f')
                            .Append(',').Append(p.FrameCount).Append(',').Append(p.FrameTick);
                    string key = Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(hash.ToString())));
                    if (!byFingerprint.TryGetValue(key, out var existing))
                        byFingerprint[key] = composition;
                    else if (!existing.Source.Contains(name, StringComparison.OrdinalIgnoreCase))
                        byFingerprint[key] = existing with { Source = existing.Source + " + " + name };
                }
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException)
            { warnings.Add(Path.GetFileName(path) + ": " + ex.Message); }
        }
        var list = byFingerprint.Values.OrderBy(c => c.Section).ThenBy(c => c.Source, StringComparer.OrdinalIgnoreCase).ToArray();
        var categories = sources.Select(Prefix).Distinct(StringComparer.OrdinalIgnoreCase).ToArray();
        return new(list, sources, warnings, categories);
    }

    /// <summary>Import a single .set file chosen in the UI (read-only: never writes to the client folder).</summary>
    public static SetCompositionTable Import(SetCompositionTable current, string path,
        CancellationToken cancellation = default)
    {
        string name = Path.GetFileName(path);
        var additions = Parse(path, cancellation);
        var seen = new HashSet<(string Source, int Section, int Count)>(
            current.Compositions.Select(c => (c.Source, c.Section, c.Pieces.Count)));
        var merged = new List<SetComposition>(current.Compositions);
        foreach (var c in additions)
        {
            var key = (c.Source, c.Section, c.Pieces.Count);
            if (!seen.Add(key)) continue;
            merged.Add(c);
        }
        var sources = current.Sources.Contains(name, StringComparer.OrdinalIgnoreCase)
            ? current.Sources : current.Sources.Append(name).ToArray();
        var categories = sources.Select(Prefix).Distinct(StringComparer.OrdinalIgnoreCase).ToArray();
        return current with
        {
            Compositions = merged.OrderBy(c => c.Section).ThenBy(c => c.Source, StringComparer.OrdinalIgnoreCase).ToArray(),
            Sources = sources, Categories = categories
        };
    }
}
