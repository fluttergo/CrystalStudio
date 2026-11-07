using System.IO.Compression;
namespace CrystalStudio;

/// <summary>Legend of Mir standard 256-colour palette in BGRA order (index 0 is transparent).</summary>
internal static class MirPalette
{
    // Generated from the byte-verified Python decoder (mir_map_editor/core/wzl_lib.py :: MIR_PALETTE).
    // Each entry is B,G,R,A -- CrystalStudio RasterFormats.Bgra32 requires this byte order.
    internal static readonly byte[] Bgra = Convert.FromHexString(
        "00000000000080FF008000FF008080FF800000FF800080FF808000FFC0C0C0FF978055FFC8B99DFF73737BFF29292DFF" +
        "52525AFF5A5A63FF393942FF18181DFF101018FF181829FF080810FF7179F2FF5F67E1FF5A5AFFFF3131FFFF525AD6FF" +
        "001094FF182994FF000839FF001073FF0018B5FF5263BDFF101842FF99AAFFFF00105AFF293973FF314AA5FF737B94FF" +
        "3152BDFF102152FF18317BFF10182DFF314A8CFF002994FF0031BDFF5273C6FF18316BFF426BC6FF004ACEFF3963A5FF" +
        "18315AFF00102AFF000815FF00183AFF000008FF000029FF00004AFF00009DFF0000DCFF0000DEFF0000FBFF52739CFF" +
        "4A6B94FF294A73FF183152FF184A8CFF114488FF00214AFF101821FF5A94D6FF216BC6FF006BEFFF0077FFFF8494A5FF" +
        "213142FF081018FF081829FF001021FF182939FF39638CFF102942FF18426BFF184A7BFF004A94FF7B848CFF5A636BFF" +
        "39424AFF182129FF293946FF94A5B5FF5A6B7BFF94B1CEFF738CA5FF5A738CFF7394B5FF73A5D6FF4AA5EFFF8CC6EFFF" +
        "42637BFF39566BFF5A94BDFF003963FFADC6D6FF294252FF186394FFADD6EFFF638CA5FF4A5A63FF7BA5BDFF18425AFF" +
        "318CBDFF293135FF638494FF4A6B7BFF5A8CA5FF294A5AFF397B9CFF103142FF21ADEFFF001018FF002129FF006B9CFF" +
        "5A8494FF184252FF295A6BFF21637BFF217B9CFF00A5DEFF39525AFF102931FF7BBDCEFF395A63FF4A8494FF29A5C6FF" +
        "189C10FF4A8C42FF428C31FF299410FF101808FF181808FF102908FF294218FFADB5A5FF73736BFF292918FF4A4218FF" +
        "4A4231FFDEC663FFFFDD44FFEFD68CFF396B73FF39DEF7FF8CEFF7FF00E7F7FF5A6B6BFFA58C5AFFEFB539FFCE9C4AFF" +
        "B58431FF6B5231FFD6DEDEFFB5BDBDFF848C8CFFDEF7F7FF180800FF391808FF291008FF001808FF002908FFA55200FF" +
        "DE7B00FF4A2910FF6B3910FF8C5210FFA55A21FF5A3110FF844210FF845231FF312118FF7B5A4AFFA56B52FF633929FF" +
        "DE4A10FF212929FF394A4AFF182929FF294A4AFF427B7BFF4A9C9CFF295A5AFF144242FF003939FF005959FF2C35CAFF" +
        "21736BFF003129FF103931FF183931FF004A42FF186352FF29735AFF184A31FF002118FF003118FF103918FF4A8463FF" +
        "4ABD6BFF4AB563FF4ABD63FF4A9C5AFF398C4AFF4AC663FF4AD663FF4A8452FF297331FF5AC663FF4ABD52FF00FF10FF" +
        "182918FF4A884AFF4AE74AFF005A00FF008800FF009400FF00DE00FF00EE00FF00FB00FF945A4AFFB57363FFD68C7BFF" +
        "D67B6BFFFF8877FFCEC6C6FF9C9494FFC6949CFF393131FF841829FF840018FF52424AFF7B4252FF735A63FFF7B5CEFF" +
        "9C7B8CFFCC2277FFFFAADDFF2AB4F0FF9F00DFFFB317E3FFF0FBFFFFA4A0A0FF808080FF0000FFFF00FF00FF00FFFFFF" +
        "FF0000FFFF00FFFFFFFF00FFFFFFFFFF"
        );
}

internal static class SpritePixels
{
    /// <summary>Read an index file while another tool (resource editor / client) has it open.</summary>
    internal static byte[] ReadShared(string path)
    {
        using var stream = new FileStream(path, FileMode.Open, FileAccess.Read,
            FileShare.ReadWrite | FileShare.Delete);
        byte[] bytes = new byte[stream.Length];
        stream.ReadExactly(bytes);
        return bytes;
    }

    /// <summary>Open a pixel file with the same shared-read policy.</summary>
    internal static FileStream OpenShared(string path) =>
        new(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);

    /// <summary>8-bit palette indices -> straight BGRA. Index 0 is the transparent colour key.</summary>
    internal static byte[] FromPalette(ReadOnlySpan<byte> indices, int pixelCount)
    {
        byte[] output = new byte[checked(pixelCount * 4)];
        byte[] palette = MirPalette.Bgra;
        int n = Math.Min(pixelCount, indices.Length);
        for (int i = 0; i < n; i++)
        {
            int o = i * 4, p = indices[i] * 4;
            output[o] = palette[p]; output[o + 1] = palette[p + 1];
            output[o + 2] = palette[p + 2];
            output[o + 3] = indices[i] == 0 ? (byte)0 : (byte)255;
        }
        return output;
    }

    /// <summary>RGB565 little-endian -> straight BGRA. 0x0000 pure black is the transparent key.</summary>
    internal static byte[] FromRgb565(ReadOnlySpan<byte> data, int pixelCount, bool bitReplication)
    {
        byte[] output = new byte[checked(pixelCount * 4)];
        int n = Math.Min(pixelCount, data.Length / 2);
        for (int i = 0; i < n; i++)
        {
            int v = data[i * 2] | (data[i * 2 + 1] << 8);
            int o = i * 4;
            if (v == 0) continue; // transparent, output stays zero
            int r5 = (v >> 11) & 0x1F, g6 = (v >> 5) & 0x3F, b5 = v & 0x1F;
            int r, g, b;
            if (bitReplication) { r = (r5 << 3) | (r5 >> 2); g = (g6 << 2) | (g6 >> 4); b = (b5 << 3) | (b5 >> 2); }
            else { r = r5 * 255 / 31; g = g6 * 255 / 63; b = b5 * 255 / 31; }
            output[o] = (byte)b; output[o + 1] = (byte)g; output[o + 2] = (byte)r; output[o + 3] = 255;
        }
        return output;
    }

    /// <summary>Decompress a zlib (RFC1950) stream. ZLibStream is available on .NET 6+.</summary>
    internal static byte[] InflateZLib(byte[] compressed, int expected)
    {
        try
        {
            using var source = new MemoryStream(compressed, false);
            using var zlib = new ZLibStream(source, CompressionMode.Decompress);
            byte[] output = new byte[expected];
            zlib.ReadExactly(output);
            return output;
        }
        catch (Exception ex) when (ex is IOException or InvalidDataException or ArgumentException)
        { throw new InvalidDataException("Invalid zlib sprite data.", ex); }
    }
}
