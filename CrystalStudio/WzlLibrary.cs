using System.IO;
using System.Buffers.Binary;

namespace CrystalStudio;

/// <summary>
/// Shanda .wzl/.wzx sprite library. Byte layout verified against the Python decoder in
/// mir_map_editor/core/wzl_lib.py and real client samples (D:\mir13\data\Tiles.wzx etc.).
///
/// WZX: 19-byte "www.shandagames.com" title, Int32 count @44, Int32[count] offsets @48.
/// WZL: at each offset a 16-byte image header followed by `size` bytes of pixel data:
///      +0 pixelFormat (3 = 8-bit palette, 5 = RGB565), +1 compressed (zlib),
///      +4 Int16 width, +6 Int16 height, +8 Int16 x, +10 Int16 y, +12 Int32 size.
/// Pixel scanlines are stored bottom-up and are flipped to top-down on decode.
/// Output is straight BGRA32 because RasterFormats.Bgra32 is what CrystalStudio renders.
/// </summary>
public sealed class WzlLibrary : IAssetLibrary
{
    private const int MaxSprites = 2_000_000;
    private static readonly byte[] HeaderTitle = "www.shandagames.com"u8.ToArray();

    private readonly int[] offsets;
    public string Path { get; }
    public int Count => offsets.Length;

    public WzlLibrary(string wzxPath, string? wzlPath = null)
    {
        string index = System.IO.Path.GetFullPath(wzxPath);
        Path = System.IO.Path.GetFullPath(wzlPath ?? System.IO.Path.ChangeExtension(index, ".wzl"));
        if (!File.Exists(Path)) throw new InvalidDataException("WZL pixel file not found: " + Path);

        byte[] bytes = SpritePixels.ReadShared(index);
        if (bytes.Length < 48) throw new InvalidDataException("WZX index too short: " + index);

        // The 44-byte title area holds a copyright string, NOT a format magic. Repacked
        // Shanda clients blank it out, and others prepend a 0x13 Pascal length byte.
        // Only count@44 followed by count Int32 offsets@48 actually defines the format,
        // so recognise all three header styles and validate the structure instead.
        bool standard = bytes.AsSpan(0, HeaderTitle.Length).SequenceEqual(HeaderTitle);
        bool prefixed = bytes[0] == HeaderTitle.Length
                        && bytes.AsSpan(1, HeaderTitle.Length).SequenceEqual(HeaderTitle);
        bool untitled = bytes.AsSpan(0, 44).IndexOfAnyExcept((byte)0) < 0;
        if (!standard && !prefixed && !untitled)
            throw new InvalidDataException("Unrecognised WZX header: " + index);

        int count = BinaryPrimitives.ReadInt32LittleEndian(bytes.AsSpan(44, 4));
        if (count < 0 || count > MaxSprites || 48L + count * 4L > bytes.Length)
            throw new InvalidDataException("Invalid WZX image count.");
        offsets = new int[count];
        for (int i = 0; i < count; i++) offsets[i] = BinaryPrimitives.ReadInt32LittleEndian(bytes.AsSpan(48 + i * 4, 4));
    }

    private (byte[] Pixels, int Width, int Height, int X, int Y)? DecodeCore(BinaryReader reader, int index)
    {
        if (index < 0 || index >= Count) return null;
        long offset = offsets[index];
        if (offset < 0 || offset + 16 > reader.BaseStream.Length) return null;
        reader.BaseStream.Position = offset;
        Span<byte> head = stackalloc byte[16];
        reader.BaseStream.ReadExactly(head);

        byte pixelFormat = head[0], compressed = head[1];
        int w = BinaryPrimitives.ReadInt16LittleEndian(head.Slice(4, 2));
        int h = BinaryPrimitives.ReadInt16LittleEndian(head.Slice(6, 2));
        int x = BinaryPrimitives.ReadInt16LittleEndian(head.Slice(8, 2));
        int y = BinaryPrimitives.ReadInt16LittleEndian(head.Slice(10, 2));
        int size = BinaryPrimitives.ReadInt32LittleEndian(head.Slice(12, 4));
        if (w <= 0 || h <= 0 || w > 8192 || h > 8192 || size <= 0 || size > reader.BaseStream.Length - reader.BaseStream.Position)
            return null; // empty sprite or corrupt/trailing index

        int pixelCount = checked(w * h);
        int expected = pixelFormat == 3 ? pixelCount : pixelFormat == 5 ? checked(pixelCount * 2) : 0;
        if (expected <= 0) return null;

        byte[] stored = new byte[size];
        reader.BaseStream.ReadExactly(stored);
        byte[] data = compressed != 0 ? SpritePixels.InflateZLib(stored, expected) : stored;
        if (data.Length < expected) return null;

        // Shanda WZL stores scanlines bottom-up. The reference decoder
        // (WeMadeLibrary.cs) writes them with `for (y = Height-1; y >= 0; y--)`,
        // and the converted Crystal .lib is top-down. Without this flip every
        // sprite is upside down: lamp posts show their base on top, tall building
        // slices no longer join, and only near-symmetric floor tiles look right.
        int bytesPerPixel = pixelFormat == 3 ? 1 : 2;
        int stride = w * bytesPerPixel;
        byte[] ordered = new byte[expected];
        for (int row = 0; row < h; row++)
            Buffer.BlockCopy(data, row * stride, ordered, (h - 1 - row) * stride, stride);

        byte[] pixels = pixelFormat == 3
            ? SpritePixels.FromPalette(ordered, pixelCount)
            : SpritePixels.FromRgb565(ordered, pixelCount, bitReplication: true);
        return (pixels, w, h, x, y);
    }

    /// <summary>Read the 16-byte WZL image header without decompressing pixel data.</summary>
    public SpriteHeader? ReadHeader(int index)
    {
        if (index < 0 || index >= Count) return null;
        try
        {
            using var stream = SpritePixels.OpenShared(Path);
            long offset = offsets[index];
            if (offset < 0 || offset + 16 > stream.Length) return null;
            stream.Position = offset;
            Span<byte> head = stackalloc byte[16];
            stream.ReadExactly(head);
            int w = BinaryPrimitives.ReadInt16LittleEndian(head.Slice(4, 2));
            int h = BinaryPrimitives.ReadInt16LittleEndian(head.Slice(6, 2));
            int x = BinaryPrimitives.ReadInt16LittleEndian(head.Slice(8, 2));
            int y = BinaryPrimitives.ReadInt16LittleEndian(head.Slice(10, 2));
            if (w <= 0 || h <= 0 || w > 8192 || h > 8192) return null;
            return new(w, h, x, y);
        }
        catch (IOException) { return null; }
        catch (UnauthorizedAccessException) { return null; }
    }

    public Sprite? Decode(int index)
    {
        using var reader = new BinaryReader(SpritePixels.OpenShared(Path));
        var decoded = DecodeCore(reader, index);
        if (decoded is null) return null;
        var (pixels, w, h, x, y) = decoded.Value;
        var bitmap = RasterImage.Create(w, h, 96, 96, RasterFormats.Bgra32, null, pixels, w * 4);
        return new Sprite(bitmap, x, y);
    }

    public Sprite? DecodeOverview(BinaryReader reader, int index, int level, CancellationToken cancellation = default)
    {
        cancellation.ThrowIfCancellationRequested();
        var decoded = DecodeCore(reader, index);
        if (decoded is null) return null;
        var (pixels, w, h, x, y) = decoded.Value;
        int targetWidth = Math.Max(1, (w + (1 << level) - 1) >> level);
        int targetHeight = Math.Max(1, (h + (1 << level) - 1) >> level);
        byte[] reduced = OverviewPixels.Reduce(pixels, w, h, targetWidth, targetHeight, cancellation);
        var bitmap = RasterImage.Create(targetWidth, targetHeight, 96, 96, RasterFormats.Bgra32, null, reduced, targetWidth * 4);
        return new Sprite(bitmap, x, y, w, h);
    }
}
