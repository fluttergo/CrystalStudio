using System;
using System.IO;
using System.Text;

namespace CrystalStudio;

/// <summary>
/// The classic Shanda / Wemade Mir2 map layout used by the official client, domestic map
/// editors (GM工具箱 etc.) and Mir2 servers such as 996: a 52-byte header followed by
/// 12 bytes per cell, column-major (x outer, y inner). <see cref="Formats.MapReader"/>'s
/// LoadMapType0 is the matching reader.
///
/// CrystalStudio's own "C#" format is an <b>editing</b> format: it stores 26 bytes per cell
/// plus a trailing placed-stamp metadata block. Game clients and domestic editors cannot
/// load it, which is why saving a client map used to break the game. When a classic map is
/// opened, Save now writes this layout back and keeps every untouched cell byte-for-byte.
/// </summary>
internal static class LegacyMapWriter
{
    public const int HeaderSize = 52;
    public const int CellSize = 12;

    /// <summary>Recognise the classic layout by its exact size and "Legend of mir" title.</summary>
    public static bool IsClassicImage(byte[] image, int width, int height)
    {
        if (image.Length < HeaderSize || width <= 0 || height <= 0) return false;
        if (image.Length != HeaderSize + (long)width * height * CellSize) return false;
        if (image[2] == (byte)'C' && image[3] == (byte)'#') return false;   // CrystalStudio format
        return HasClassicTitle(image);
    }

    private static bool HasClassicTitle(byte[] b) =>
        b[4] == 13 && b[5] == (byte)'L' && b[6] == (byte)'e' && b[7] == (byte)'g' &&
        b[8] == (byte)'e' && b[9] == (byte)'n' && b[10] == (byte)'d' && b[11] == (byte)' ' &&
        b[12] == (byte)'o' && b[13] == (byte)'f' && b[14] == (byte)' ' && b[15] == (byte)'m' &&
        b[16] == (byte)'i' && b[17] == (byte)'r';

    /// <summary>
    /// Build a header for a map that has no classic original. The reserved tail is copied from
    /// a sibling classic map when possible (official editors leave client-specific values there),
    /// otherwise it is zeroed, which the client and domestic editors both accept.
    /// </summary>
    public static byte[] CreateHeader(string targetPath, int width, int height)
    {
        string? folder = Path.GetDirectoryName(Path.GetFullPath(targetPath));
        if (!string.IsNullOrEmpty(folder) && Directory.Exists(folder))
        {
            foreach (string candidate in Directory.EnumerateFiles(folder, "*.map"))
            {
                if (string.Equals(candidate, Path.GetFullPath(targetPath), StringComparison.OrdinalIgnoreCase)) continue;
                try
                {
                    if (File.ReadAllBytes(candidate) is not { Length: >= HeaderSize } bytes) continue;
                    if (!HasClassicTitle(bytes)) continue;
                    byte[] header = bytes[..HeaderSize];
                    BitConverter.GetBytes(checked((short)width)).CopyTo(header, 0);
                    BitConverter.GetBytes(checked((short)height)).CopyTo(header, 2);
                    return header;
                }
                catch (Exception ex) when (ex is IOException or UnauthorizedAccessException) { }
            }
        }
        byte[] fresh = new byte[HeaderSize];
        BitConverter.GetBytes(checked((short)width)).CopyTo(fresh, 0);
        BitConverter.GetBytes(checked((short)height)).CopyTo(fresh, 2);
        fresh[4] = 13;
        Encoding.ASCII.GetBytes("Legend of mir").CopyTo(fresh, 5);
        return fresh;
    }

    /// <summary>Refuse to silently drop editing features that the classic layout cannot store.</summary>
    public static void Validate(MapDocument document)
    {
        foreach (var pair in document.Cells)
        {
            Cell c = pair.Value;
            if (c.BackLibrary != 0 || c.MiddleLibrary is not (0 or 1) ||
                c.MiddleFrames != 0 || c.MiddleTick != 0 || c.TileAnimation != 0 ||
                c.TileOffset != 0 || c.TileFrames != 0 ||
                (c.BackImage & unchecked((int)0x9FFF8000)) != 0 ||
                (c.Foreground is { } front && front.Library is < 0 or > 257))
                throw new InvalidDataException(LocalText.T("This map uses features that a classic Mir2 .map cannot store"));
        }
    }

    /// <summary>
    /// Write the classic layout. <paramref name="sourceImage"/> is the original file (when the
    /// document came from one): untouched cells are copied verbatim so client-specific header
    /// bytes, door flag bit 7 and residual library/high bits are never lost.
    /// </summary>
    public static void Write(Stream stream, MapDocument document, byte[] header, byte[]? sourceImage)
    {
        Validate(document);
        using var w = new BinaryWriter(stream, Encoding.UTF8, leaveOpen: true);
        w.Write(header);
        byte[]? original = sourceImage != null ? new byte[CellSize] : null;
        for (int x = 0; x < document.Width; x++)
            for (int y = 0; y < document.Height; y++)
            {
                int index = x * document.Height + y;
                bool hasOriginal = false;
                if (sourceImage != null)
                {
                    long offset = HeaderSize + (long)index * CellSize;
                    if (offset + CellSize <= sourceImage.Length)
                    {
                        Buffer.BlockCopy(sourceImage, (int)offset, original!, 0, CellSize);
                        hasOriginal = true;
                    }
                }
                Cell cell = NormalizeForSave(document.Get(x, y), document.SourceFormat);
                byte[]? source = hasOriginal ? original : null;
                if (source != null && Equivalent(DecodeCell(source), cell)) w.Write(source);
                else w.Write(EncodeCell(cell, source));
            }
    }

    private static Cell NormalizeForSave(Cell c, AssetSourceFormat format)
    {
        if (format != AssetSourceFormat.Sanxiu || c.Foreground is not { Library: >= 2000 and < 3000 } front) return c;
        c = c.Copy();
        c.Foreground = front with { Library = (short)(front.Library - 2000) };
        return c;
    }

    /// <summary>Decode one classic cell exactly the way MapReader.LoadMapType0 does.</summary>
    internal static Cell DecodeCell(ReadOnlySpan<byte> b)
    {
        short backShort = System.Buffers.Binary.BinaryPrimitives.ReadInt16LittleEndian(b);
        short midShort = System.Buffers.Binary.BinaryPrimitives.ReadInt16LittleEndian(b[2..]);
        short frontShort = System.Buffers.Binary.BinaryPrimitives.ReadInt16LittleEndian(b[4..]);
        int back = backShort & 0xFFFF;
        if ((back & 0x8000) != 0) back = (back & 0x7FFF) | 0x20000000;
        byte door = b[6], doorOffset = b[7], frames = b[8], tick = b[9], frontRaw = b[10], light = b[11];
        bool hasFront = (frontShort & 0x7FFF) > 0 && frontRaw + 2 > 0;
        if (hasFront && (frontShort & 0x8000) != 0) back |= 0x40000000;
        var cell = new Cell
        {
            BackLibrary = 0, BackImage = back, MiddleLibrary = 1, MiddleImage = (ushort)midShort,
            Door = door, DoorOffset = doorOffset, Light = light
        };
        if (hasFront) cell.Foreground = new Front((short)(frontRaw + 2), frontShort, frames, tick);
        return cell;
    }

    private static bool Equivalent(Cell a, Cell b) =>
        a.BackImage == b.BackImage && a.MiddleImage == b.MiddleImage && a.Foreground == b.Foreground &&
        a.Door == b.Door && a.DoorOffset == b.DoorOffset && a.Light == b.Light &&
        a.MiddleFrames == b.MiddleFrames && a.MiddleTick == b.MiddleTick &&
        a.TileAnimation == b.TileAnimation && a.TileOffset == b.TileOffset && a.TileFrames == b.TileFrames;

    /// <summary>Encode one cell back into the classic 12-byte layout.</summary>
    internal static byte[] EncodeCell(Cell c, byte[]? original)
    {
        byte[] result = new byte[CellSize];
        int back = c.BackImage;
        System.Buffers.Binary.BinaryPrimitives.WriteInt16LittleEndian(result,
            unchecked((short)((back & 0x7FFF) | ((back & 0x20000000) != 0 ? 0x8000 : 0))));
        System.Buffers.Binary.BinaryPrimitives.WriteUInt16LittleEndian(result.AsSpan(2), (ushort)c.MiddleImage);

        Front? front = c.Foreground;
        bool hasFront = front is { Image: not 0 };
        int frontImage = 0;
        if (hasFront)
        {
            frontImage = front!.Image;
            if ((back & 0x40000000) != 0) frontImage |= 0x8000;
        }
        else if (original != null)
            frontImage = System.Buffers.Binary.BinaryPrimitives.ReadUInt16LittleEndian(original.AsSpan(4)) & 0x8000;
        System.Buffers.Binary.BinaryPrimitives.WriteInt16LittleEndian(result.AsSpan(4), unchecked((short)frontImage));

        // Bit 7 of the door byte is a client flag; the editor's own reader strips it on
        // Crystal-format reloads, so restore it from the original cell whenever present.
        result[6] = (byte)(c.Door | (original != null ? original[6] & 0x80 : 0));
        result[7] = c.DoorOffset;
        result[8] = hasFront ? front!.Frames : original?[8] ?? (byte)0;
        result[9] = hasFront ? front!.Tick : original?[9] ?? (byte)0;
        result[10] = hasFront
            ? (byte)Math.Clamp(front!.Library - 2, 0, 255)
            : original?[10] ?? (byte)0;
        result[11] = c.Light;
        return result;
    }
}
