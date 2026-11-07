using System.Buffers.Binary;
using System.IO;
using System.Text.Json;

namespace CrystalStudio;

/// <summary>
/// Resolves a .set IMG number to a registered library slot. Shanda .set indexes run over the
/// <b>Objects*.wzl family in fixed 10,000-image slots</b> (Objects 0..9999, Objects2 10000..19999,
/// ...), not over Tiles*. That is why a domestic editor renders complete buildings while a
/// Tiles-based or count-packed binding only produces ground tiles. A set-binding.json next to
/// the .set files can override the slot stride, supply exact IMG overrides, or force an explicit
/// cumulative segment table for non-standard clients.
/// </summary>
public interface ISetImageBinding
{
    bool TryResolve(int image, out int libraryId, out int imageIndex);
    string Description { get; }
    int SegmentCount { get; }
}

public sealed class SegmentImageBinding : ISetImageBinding
{
    private readonly (int LibraryId, int Count)[] segments;
    private readonly Dictionary<int, (int LibraryId, int ImageIndex)> overrides;
    public string Description { get; }

    public SegmentImageBinding(IEnumerable<(int LibraryId, int Count)> segments,
        IReadOnlyDictionary<int, (int LibraryId, int ImageIndex)>? overrides = null, string? description = null)
    {
        this.segments = segments.ToArray();
        this.overrides = overrides?.ToDictionary(p => p.Key, p => p.Value) ?? [];
        Description = description ?? "Cumulative Objects-family image numbers";
    }

    public int SegmentCount => segments.Length;

    public bool TryResolve(int image, out int libraryId, out int imageIndex)
    {
        libraryId = imageIndex = -1;
        if (overrides.TryGetValue(image, out var exact)) { libraryId = exact.LibraryId; imageIndex = exact.ImageIndex; return true; }
        int start = 0;
        foreach (var (id, count) in segments)
        {
            if (image >= start && image < start + count)
            {
                if (id < 0) return false;              // segment known but library not loaded
                libraryId = id; imageIndex = image - start; return true;
            }
            start += count;
        }
        return false;
    }
}

/// <summary>
/// Shanda .set IMG numbers are addressed in <b>fixed 10,000-image slots</b> over the
/// Objects.wzl family: slot 0 = Objects, slot 1 = Objects2, slot 2 = Objects3, ...
/// so IMG 30361 is Objects4 image 361. The stride is a client constant, NOT the
/// WZX image count (9999, 10024, 9229, ...). Packing segments by their real counts
/// shifts every library after the first (slot 3 ends up ~748 images off), which
/// scrambles composed buildings into scattered fragments. Missing or empty libraries
/// still own their whole slot and must never be compacted out of the table.
/// A set-binding.json can still override the stride or supply explicit segments.
/// </summary>
public sealed class SlotImageBinding : ISetImageBinding
{
    private readonly int[] slotLibraryIds;
    private readonly Dictionary<int, (int LibraryId, int ImageIndex)> overrides;
    public int SlotSize { get; }
    public string Description { get; }

    public SlotImageBinding(IEnumerable<int> slotLibraryIds, int slotSize = 10000,
        IReadOnlyDictionary<int, (int LibraryId, int ImageIndex)>? overrides = null, string? description = null)
    {
        this.slotLibraryIds = slotLibraryIds.ToArray();
        SlotSize = slotSize > 0 ? slotSize : 10000;
        this.overrides = overrides?.ToDictionary(p => p.Key, p => p.Value) ?? [];
        Description = description ?? $"Fixed {SlotSize:N0}-image slots over Objects/Objects2..N (Shanda .set IMG)";
    }

    public int SegmentCount => slotLibraryIds.Length;

    public bool TryResolve(int image, out int libraryId, out int imageIndex)
    {
        libraryId = imageIndex = -1;
        if (overrides.TryGetValue(image, out var exact)) { libraryId = exact.LibraryId; imageIndex = exact.ImageIndex; return true; }
        if (image < 0) return false;
        int slot = image / SlotSize;
        if (slot >= slotLibraryIds.Length) return false;
        int id = slotLibraryIds[slot];
        if (id < 0) return false;                 // slot known but library not loaded
        libraryId = id; imageIndex = image % SlotSize; return true;
    }
}

public sealed class SetImageBindingFile
{
    public string? Description { get; set; }
    public List<SetBindingSegment>? Segments { get; set; }
    public Dictionary<string, int[]>? Overrides { get; set; }
    /// <summary>Override the fixed slot stride (default 10000) for non-standard clients.</summary>
    public int? SlotSize { get; set; }
}
public sealed class SetBindingSegment
{
    public int LibraryId { get; set; }
    public int Count { get; set; }
}

public static class SetImageBindingFactory
{
    public const string FileName = "set-binding.json";
    /// <summary>Shanda clients reserve 10,000 IMG numbers for each Objects*.wzl slot.</summary>
    public const int DefaultSlotSize = 10000;
    private const int MaxSlots = 100;   // Objects, Objects2 .. Objects100

    /// <summary>Build a binding from the client's Objects*.wzx files and loaded library slots.</summary>
    public static ISetImageBinding Create(string dataFolder, AssetStore? assets)
    {
        int slotSize = DefaultSlotSize;
        var overrides = new Dictionary<int, (int, int)>();
        string config = Path.Combine(dataFolder, FileName);
        if (File.Exists(config))
        {
            try
            {
                // Use the source-generated contract: the NativeAOT publish profile disables
                // reflection-based JSON, so JsonSerializer.Deserialize<T>(string) would fail there.
                var doc = StudioJson.Deserialize<SetImageBindingFile>(File.ReadAllText(config),
                    new JsonSerializerOptions { PropertyNameCaseInsensitive = true });
                if (doc?.SlotSize is > 0 and <= 1_000_000) slotSize = doc.SlotSize.Value;
                if (doc?.Overrides != null)
                    foreach (var (key, value) in doc.Overrides)
                        if (int.TryParse(key, out int image) && value is { Length: >= 2 })
                            overrides[image] = (value[0], value[1]);
                // Explicit segments keep the legacy cumulative-pack behaviour for unusual clients.
                if (doc?.Segments is { Count: > 0 })
                    return new SegmentImageBinding(doc.Segments.Select(s => (s.LibraryId, s.Count)), overrides,
                        doc.Description ?? "Custom set-binding.json");
            }
            catch (Exception ex) when (ex is IOException or JsonException or UnauthorizedAccessException
                or NotSupportedException)
            { StudioLog.Warning("Set.Binding", $"Cannot read {config}; using fixed slots", ex); }
        }

        // Slot k owns IMG numbers [k*slotSize, (k+1)*slotSize). A library that is missing
        // or empty still owns its whole slot: compacting it out would shift every later
        // library and scramble compositions built from those images.
        var slots = new int[MaxSlots];
        for (int slot = 0; slot < MaxSlots; slot++)
        {
            string name = slot == 0 ? "Objects" : "Objects" + (slot + 1);
            string index = Path.Combine(dataFolder, name + ".wzx");
            string wilIndex = Path.Combine(dataFolder, name + ".wix");
            slots[slot] = File.Exists(index) || File.Exists(wilIndex) ? FindLibraryId(assets, name) : -1;
        }
        return new SlotImageBinding(slots, slotSize, overrides);
    }

    private static int FindLibraryId(AssetStore? assets, string name)
    {
        if (assets != null)
            foreach (var (id, library) in assets.Libraries.OrderBy(p => p.Key))
                if (Path.GetFileNameWithoutExtension(library.Path).Equals(name, StringComparison.OrdinalIgnoreCase))
                    return id;
        // Conventional Wemade IDs when the store is not loaded yet: Objects=2, ObjectsN=N+1.
        if (name.Equals("Objects", StringComparison.OrdinalIgnoreCase)) return 2;
        if (name.StartsWith("Objects", StringComparison.OrdinalIgnoreCase) &&
            int.TryParse(name[7..], out int number) && number is >= 2 and <= 99) return number + 1;
        return -1;
    }
}

/// <summary>Composition preview and statistics built on top of a binding strategy.</summary>
public static class SetCompositionResolver
{
    public const int CellWidth = 48;
    public const int CellHeight = 32;
    public sealed record Stats(int Total, int Bound, int Decoded);

    public static Stats Measure(SetComposition composition, AssetStore? assets, ISetImageBinding binding)
    {
        int total = 0, bound = 0, decoded = 0;
        foreach (var piece in composition.Pieces)
        {
            if (!piece.HasImage) continue;
            total++;
            if (!binding.TryResolve(piece.Image, out int libraryId, out int imageIndex)) continue;
            bound++;
            if (assets?.Libraries.TryGetValue(libraryId, out var library) == true && library.Decode(imageIndex) != null)
                decoded++;
        }
        return new(total, bound, decoded);
    }

    /// <summary>
    /// Composite the composition on the 48×32 isometric grid, in .set row order (painter's
    /// algorithm). Tile-sized sprites sit at (X*48, Y*32); tall sprites are bottom-aligned to
    /// (Y+1)*32. The whole result is then scaled down to fit maxSize, like a domestic editor's
    /// model thumbnail. Black is the thumbnail background; removeBlack turns near-black pixels
    /// transparent so the composite can be pasted over a map. Rows flagged additive in the .set
    /// (column 7 = 't') are light/flame sprites: the client draws them with SourceAlpha/One
    /// compositing and the image-header X/Y offset, so their near-black palette entries add
    /// nothing instead of painting an opaque black square. <paramref name="frame"/> selects
    /// one step of the light's animation loop (column 5 = frame count); frame 0 is the still
    /// image used by thumbnails and the main export.
    /// </summary>
    public static RasterImage? Render(SetComposition composition, AssetStore? assets,
        ISetImageBinding binding, int maxSize = 320, CancellationToken cancellation = default,
        bool removeBlack = false, int frame = 0)
    {
        if (assets == null) return null;
        var sprites = new List<(int X, int Y, int W, int H, byte[] Pixels, bool Additive)>();
        foreach (var piece in composition.Pieces)
        {
            cancellation.ThrowIfCancellationRequested();
            if (!piece.HasImage || !binding.TryResolve(piece.Image, out int libraryId, out int imageIndex)) continue;
            if (!assets.Libraries.TryGetValue(libraryId, out var library)) continue;
            int spriteIndex = imageIndex;
            if (piece.Animated)
            {
                // The frames of a light are consecutive images inside the same library, so
                // the same library id / WZL offset applies to every step of the loop.
                int count = Math.Clamp(piece.FrameCount, 1, 127);
                spriteIndex += ((frame % count) + count) % count;
            }
            var sprite = library.Decode(spriteIndex);
            if (sprite is null) continue;
            int w = sprite.Bitmap.PixelWidth, h = sprite.Bitmap.PixelHeight;
            if (w <= 0 || h <= 0 || w > 2048 || h > 2048) continue;
            byte[] pixels = new byte[w * h * 4];
            sprite.Bitmap.CopyPixels(pixels, w * 4, 0);
            int px, py;
            if (piece.Additive)
            {
                // Reuse the map renderer's blend placement so thumbnails / exported PNGs and
                // cells dropped on a map agree exactly, including the WZL header offset.
                var bounds = MapSpriteDrawing.Bounds(sprite, piece.X, piece.Y, libraryId, spriteIndex,
                    anchored: true, front: true, blend: true, format: assets.Format);
                px = (int)Math.Round(bounds.X);
                py = (int)Math.Round(bounds.Y);
            }
            else
            {
                bool tile = (w == CellWidth && h == CellHeight) || (w == 2 * CellWidth && h == 2 * CellHeight);
                px = piece.X * CellWidth;
                py = tile ? piece.Y * CellHeight : (piece.Y + 1) * CellHeight - h;
            }
            sprites.Add((px, py, w, h, pixels, piece.Additive));
            if (sprites.Count >= 2048) break;   // guard against pathological compositions
        }
        if (sprites.Count == 0) return null;
        int left = sprites.Min(s => s.X), top = sprites.Min(s => s.Y);
        int right = sprites.Max(s => s.X + s.W), bottom = sprites.Max(s => s.Y + s.H);
        int spanW = Math.Max(1, right - left), spanH = Math.Max(1, bottom - top);
        double scale = Math.Min(1.0, Math.Min((double)maxSize / spanW, (double)maxSize / spanH));
        int canvasW = Math.Max(1, (int)Math.Round(spanW * scale));
        int canvasH = Math.Max(1, (int)Math.Round(spanH * scale));
        byte[] canvas = new byte[canvasW * canvasH * 4];
        for (int i = 0; i < canvasW * canvasH; i++) canvas[i * 4 + 3] = 255;  // opaque black backdrop
        foreach (var s in sprites)
        {
            for (int y = 0; y < s.H; y++)
            {
                int dy = (int)Math.Round((s.Y + y - top) * scale);
                if (dy < 0 || dy >= canvasH) continue;
                for (int x = 0; x < s.W; x++)
                {
                    int dx = (int)Math.Round((s.X + x - left) * scale);
                    if (dx < 0 || dx >= canvasW) continue;
                    int src = (y * s.W + x) * 4, dst = (dy * canvasW + dx) * 4;
                    int alpha = s.Pixels[src + 3];
                    if (alpha == 0) continue;
                    if (s.Additive)
                    {
                        // SourceAlpha/One: premultiply by the source alpha and add. Near-black
                        // palette entries therefore leave the backdrop untouched.
                        for (int c = 0; c < 3; c++)
                            canvas[dst + c] = (byte)Math.Min(255,
                                canvas[dst + c] + s.Pixels[src + c] * alpha / 255);
                        canvas[dst + 3] = 255;
                    }
                    else if (alpha == 255)
                    {
                        canvas[dst] = s.Pixels[src]; canvas[dst + 1] = s.Pixels[src + 1];
                        canvas[dst + 2] = s.Pixels[src + 2]; canvas[dst + 3] = 255;
                    }
                    else
                    {
                        double a = alpha / 255.0;
                        for (int c = 0; c < 3; c++)
                            canvas[dst + c] = (byte)Math.Clamp(s.Pixels[src + c] * a + canvas[dst + c] * (1 - a), 0, 255);
                        canvas[dst + 3] = (byte)Math.Clamp(alpha + canvas[dst + 3] * (1 - a), 0, 255);
                    }
                }
            }
        }
        if (removeBlack)
            for (int i = 0; i < canvasW * canvasH; i++)
            {
                int o = i * 4;
                if (canvas[o] <= 16 && canvas[o + 1] <= 16 && canvas[o + 2] <= 16) canvas[o + 3] = 0;
            }
        return RasterImage.Create(canvasW, canvasH, 96, 96, RasterFormats.Bgra32, null, canvas, canvasW * 4);
    }

    /// <summary>
    /// Expand a composition into stamp cells. X/Y already are map-cell coordinates, so they
    /// are used verbatim. Every resolved sprite belongs to the Objects family and is therefore
    /// written to the cell's foreground layer; when several pieces share a cell the last row
    /// wins (painter order = topmost). Front.Image is 1-based in the map format. Rows flagged
    /// additive in the .set get Front.Frames bit 0x80, which the map renderer reads via
    /// MapSpriteDrawing.FrontBlend to switch to additive compositing (and the header offset).
    /// Without it a light sprite paints its near-black palette entries as an opaque black square.
    /// The low 7 bits of Front.Frames hold the .set frame count and Front.Tick the frame delay:
    /// MapViewport advances the image index by animationFrame / (1 + Tick) % Frames every 100 ms,
    /// so a dropped light becomes a genuinely animated cell instead of a frozen first frame.
    /// </summary>
    public static List<StampCell> ToStampCells(SetComposition composition, ISetImageBinding binding)
    {
        var cells = new Dictionary<(int X, int Y), (int LibraryId, int ImageIndex, byte Frames, byte Tick)>();
        foreach (var piece in composition.Pieces)
        {
            if (!piece.HasImage || !binding.TryResolve(piece.Image, out int libraryId, out int imageIndex)) continue;
            if (imageIndex + 1 > short.MaxValue) continue;
            // Front.Frames: 0x80 = additive blend, 0x7F = animation frame count.
            byte frames = piece.Additive
                ? (byte)(0x80 | Math.Clamp(piece.FrameCount, 0, 0x7F)) : (byte)0;
            byte tick = piece.Additive ? (byte)Math.Clamp(piece.FrameTick, 0, 255) : (byte)0;
            cells[(piece.X, piece.Y)] = (libraryId, imageIndex, frames, tick);
        }
        return cells.Select(p => new StampCell(p.Key.X, p.Key.Y,
            new Cell { Foreground = new Front((short)p.Value.LibraryId, (short)(p.Value.ImageIndex + 1),
                p.Value.Frames, p.Value.Tick) })).ToList();
    }
}
