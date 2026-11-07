using System.IO;
using System.Buffers.Binary;

namespace CrystalStudio;

/// <summary>
/// Wemade .wil/.wix sprite library. Byte layout verified in mir_map_editor/core/wil_lib.py.
///
/// WIX: 36-byte "#INDX v1.0-WEMADE Entertainment inc." title, UInt32 count @44,
///      UInt32[count] offsets @48.
/// WIL: 36-byte "#ILIB v1.0-WEMADE Entertainment inc." title, 256x4 BGRA palette @56,
///      image data from 1080. Each image: UInt16 w, UInt16 h, Int16 x, Int16 y, then pixels.
///      Two client variants exist (both verified on real files):
///        - length == w*h      : 8-bit indices into the embedded palette (mir10 Tiles.wil)
///        - length == w*h*2    : RGB565 little-endian, 0x0000 transparent (Wemade Tiles139)
///      Shorter payloads are treated as (count:u16, colour:u16) RLE.
///      Output is always straight BGRA32; the embedded palette is already in that byte order.
/// </summary>
public sealed class WilLibrary : IAssetLibrary
{
    private const int MaxSprites = 2_000_000;
    private const int PaletteOffset = 56;
    private const int PaletteBytes = 256 * 4;
    private const int DataStart = PaletteOffset + PaletteBytes;

    private readonly long[] offsets;
    private readonly byte[] palette;
    public string Path { get; }
    public int Count => offsets.Length;

    public WilLibrary(string wixPath, string? wilPath = null)
    {
        string index = System.IO.Path.GetFullPath(wixPath);
        Path = System.IO.Path.GetFullPath(wilPath ?? System.IO.Path.ChangeExtension(index, ".wil"));
        if (!File.Exists(Path)) throw new InvalidDataException("WIL pixel file not found: " + Path);

        byte[] bytes = SpritePixels.ReadShared(index);
        if (bytes.Length < 48) throw new InvalidDataException("WIX index is too small: " + index);
        // Older/newer Wemade variants use different titles; the offset table layout is the same.
        long declared = BinaryPrimitives.ReadUInt32LittleEndian(bytes.AsSpan(44, 4));
        if (declared > MaxSprites) throw new InvalidDataException("Invalid WIX image count.");
        int count = (int)declared;
        if (48L + count * 4L > bytes.Length) count = Math.Max(0, (bytes.Length - 48) / 4);
        offsets = new long[count];
        for (int i = 0; i < count; i++) offsets[i] = BinaryPrimitives.ReadUInt32LittleEndian(bytes.AsSpan(48 + i * 4, 4));

        using var image = new BinaryReader(SpritePixels.OpenShared(Path));
        if (image.BaseStream.Length < DataStart) throw new InvalidDataException("WIL pixel file is too small: " + Path);
        image.BaseStream.Position = PaletteOffset;
        palette = image.ReadBytes(PaletteBytes);
        if (palette.Length != PaletteBytes) throw new InvalidDataException("WIL palette is truncated: " + Path);
    }

    private byte[] FromEmbeddedPalette(byte[] indices, int pixelCount)
    {
        byte[] output = new byte[checked(pixelCount * 4)];
        int n = Math.Min(pixelCount, indices.Length);
        for (int i = 0; i < n; i++)
        {
            int p = indices[i] * 4, o = i * 4;
            output[o] = palette[p]; output[o + 1] = palette[p + 1];
            output[o + 2] = palette[p + 2]; output[o + 3] = palette[p + 3];
        }
        return output;
    }

    private (byte[] Pixels, int Width, int Height, int X, int Y)? DecodeCore(BinaryReader reader, int index)
    {
        if (index < 0 || index >= Count) return null;
        long length = reader.BaseStream.Length;
        long offset = offsets[index];
        long next = index + 1 < Count ? offsets[index + 1] : length;
        if (offset < 0 || offset + 8 > length) return null;
        reader.BaseStream.Position = offset;
        Span<byte> head = stackalloc byte[8];
        reader.BaseStream.ReadExactly(head);
        int w = BinaryPrimitives.ReadUInt16LittleEndian(head.Slice(0, 2));
        int h = BinaryPrimitives.ReadUInt16LittleEndian(head.Slice(2, 2));
        int x = BinaryPrimitives.ReadInt16LittleEndian(head.Slice(4, 2));
        int y = BinaryPrimitives.ReadInt16LittleEndian(head.Slice(6, 2));
        if (w <= 0 || h <= 0 || w > 8192 || h > 8192) return null;

        long dataLength = Math.Min(next, length) - offset - 8;
        if (dataLength <= 0 || dataLength > length - reader.BaseStream.Position) return null;
        byte[] stored = new byte[(int)dataLength];
        reader.BaseStream.ReadExactly(stored);

        int pixelCount = checked(w * h);
        int expected565 = checked(pixelCount * 2);
        byte[] pixels;
        if (dataLength == pixelCount)
        {
            // 8-bit indexed variant: the WIL header carries its own 256-colour BGRA palette.
            pixels = FromEmbeddedPalette(stored, pixelCount);
        }
        else if (dataLength == expected565)
        {
            pixels = SpritePixels.FromRgb565(stored, pixelCount, bitReplication: false);
        }
        else if (dataLength < expected565)
        {
            byte[]? expanded = ExpandRle(stored, expected565);
            if (expanded is null) return null;
            pixels = SpritePixels.FromRgb565(expanded, pixelCount, bitReplication: false);
        }
        else
        {
            pixels = SpritePixels.FromRgb565(stored.AsSpan(0, expected565), pixelCount, bitReplication: false);
        }
        // Mir2 stores WIL scanlines bottom-up too: the reference WeMadeLibrary
        // decoder writes y = Height-1 .. 0 for every nType (WIL, WZL, MIZ...).
        // Flip the finished BGRA rows so WIL artwork matches WZL and Crystal .lib.
        int stride = w * 4, total = checked(h * stride);
        if (pixels.Length >= total)
        {
            byte[] ordered = new byte[total];
            for (int row = 0; row < h; row++)
                Buffer.BlockCopy(pixels, row * stride, ordered, (h - 1 - row) * stride, stride);
            pixels = ordered;
        }
        return (pixels, w, h, x, y);
    }

    private static byte[]? ExpandRle(byte[] data, int expectedBytes)
    {
        byte[] output = new byte[expectedBytes];
        int written = 0;
        for (int i = 0; i + 4 <= data.Length; i += 4)
        {
            int count = BinaryPrimitives.ReadUInt16LittleEndian(data.AsSpan(i, 2));
            ushort colour = BinaryPrimitives.ReadUInt16LittleEndian(data.AsSpan(i + 2, 2));
            if (count == 0) break;
            byte lo = (byte)(colour & 0xFF), hi = (byte)(colour >> 8);
            for (int n = 0; n < count && written + 2 <= expectedBytes; n++)
            { output[written++] = lo; output[written++] = hi; }
            if (written >= expectedBytes) break;
        }
        return written >= expectedBytes ? output : null;
    }

    /// <summary>Read the 8-byte WIL image header without decoding pixel data.</summary>
    public SpriteHeader? ReadHeader(int index)
    {
        if (index < 0 || index >= Count) return null;
        try
        {
            using var stream = SpritePixels.OpenShared(Path);
            long offset = offsets[index];
            if (offset < 0 || offset + 8 > stream.Length) return null;
            stream.Position = offset;
            Span<byte> head = stackalloc byte[8];
            stream.ReadExactly(head);
            int w = BinaryPrimitives.ReadUInt16LittleEndian(head.Slice(0, 2));
            int h = BinaryPrimitives.ReadUInt16LittleEndian(head.Slice(2, 2));
            int x = BinaryPrimitives.ReadInt16LittleEndian(head.Slice(4, 2));
            int y = BinaryPrimitives.ReadInt16LittleEndian(head.Slice(6, 2));
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
