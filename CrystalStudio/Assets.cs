using System.IO;
using System.IO.Compression;
using System.Collections.Concurrent;
using System.Buffers;
using System.Threading.Channels;
namespace CrystalStudio;

public record Sprite(RasterImage Bitmap, int X, int Y, int OriginalWidth = 0, int OriginalHeight = 0)
{
    public int Width => OriginalWidth > 0 ? OriginalWidth : Bitmap.PixelWidth;
    public int Height => OriginalHeight > 0 ? OriginalHeight : Bitmap.PixelHeight;
}
public readonly record struct SpriteHeader(int Width, int Height, int X, int Y);
public sealed class CrystalLibrary : IAssetLibrary
{
    public string Path { get; }
    public int Count => offsets.Length;
    private readonly int[] offsets;
    private readonly ModernImage[]? modernImages;
    private readonly record struct ModernImage(SpriteHeader Header, int Length, byte FileType);

    public CrystalLibrary(string path, bool allowSanxiu = false)
    {
        Path = System.IO.Path.GetFullPath(path);
        using var r = new BinaryReader(File.OpenRead(Path));
        int version = r.ReadInt32();
        if (allowSanxiu && version is 5 or 6)
        {
            if (r.ReadByte() != 0) throw new InvalidDataException("Only unencrypted Sanxiu v5/v6 libraries are supported: " + Path);
            int headerLength = r.ReadInt32();
            if (headerLength < 4 || headerLength > r.BaseStream.Length - r.BaseStream.Position || headerLength > 128 * 1024 * 1024)
                throw new InvalidDataException("Invalid Sanxiu library header.");
            byte[] header = r.ReadBytes(headerLength);
            using var metadata = new BinaryReader(new MemoryStream(header));
            int imageCount = metadata.ReadInt32();
            if (imageCount < 0 || imageCount > 2_000_000 || (long)imageCount * (version == 6 ? 18 : 17) > header.Length - 4)
                throw new InvalidDataException("Invalid Sanxiu image count.");
            offsets = new int[imageCount];
            modernImages = new ModernImage[imageCount];
            long offset = r.BaseStream.Position;
            for (int i = 0; i < imageCount; i++)
            {
                int width = metadata.ReadInt16(), height = metadata.ReadInt16();
                int x = metadata.ReadInt16(), y = metadata.ReadInt16();
                metadata.ReadInt16(); metadata.ReadInt16();
                byte shadow = metadata.ReadByte();
                byte fileType = version == 6 ? metadata.ReadByte() : (byte)0;
                int length = metadata.ReadInt32(), maskLength = 0;
                if ((shadow & 0x80) != 0)
                {
                    metadata.ReadInt16(); metadata.ReadInt16(); metadata.ReadInt16(); metadata.ReadInt16();
                    maskLength = metadata.ReadInt32();
                }
                if (length < 0 || maskLength < 0 || offset > int.MaxValue || offset + length + maskLength > r.BaseStream.Length)
                    throw new InvalidDataException("Invalid Sanxiu sprite offset or length.");
                offsets[i] = (int)offset;
                modernImages[i] = new(new(width, height, x, y), length, fileType);
                offset += (long)length + maskLength;
            }
            return;
        }
        if (allowSanxiu && version != 3) throw new InvalidDataException("Expected a Sanxiu v3, v5, or v6 library: " + Path);
        int count = r.ReadInt32();
        if ((version != 2 && version != 3) || count < 0 || count > 2_000_000 || (version == 3 ? 12L : 8L) + count * 4L > r.BaseStream.Length) throw new InvalidDataException("Expected a Crystal version 2 or 3 library.");
        if (version == 3) r.ReadInt32();
        offsets = new int[count]; for (int i = 0; i < count; i++) { offsets[i] = r.ReadInt32(); if (offsets[i] < 0 || offsets[i] >= r.BaseStream.Length) throw new InvalidDataException("Invalid sprite offset."); }
    }
    public SpriteHeader? ReadHeader(int index)
    {
        if (index < 0 || index >= Count) return null;
        if (modernImages is { } images)
        {
            var header = images[index].Header;
            if (images[index].FileType == 1 && (header.Width <= 0 || header.Height <= 0) && images[index].Length >= 10)
            {
                using var stream = File.OpenRead(Path);
                stream.Position = offsets[index];
                Span<byte> gifHeader = stackalloc byte[10];
                stream.ReadExactly(gifHeader);
                if (gifHeader[..3].SequenceEqual("GIF"u8))
                    header = new(gifHeader[6] | gifHeader[7] << 8, gifHeader[8] | gifHeader[9] << 8, header.X, header.Y);
            }
            return header.Width is > 0 and <= 8192 && header.Height is > 0 and <= 8192 ? header : null;
        }
        using var r = new BinaryReader(File.OpenRead(Path)); r.BaseStream.Position = offsets[index];
        int w = r.ReadInt16(), h = r.ReadInt16(), x = r.ReadInt16(), y = r.ReadInt16();
        r.ReadInt16(); r.ReadInt16(); r.ReadByte(); int length = r.ReadInt32();
        if (w <= 0 || h <= 0) return null;
        if (w > 8192 || h > 8192 || length < 0 || length > r.BaseStream.Length - r.BaseStream.Position)
            throw new InvalidDataException("Invalid sprite dimensions or length.");
        return new(w, h, x, y);
    }
    public Sprite? Decode(int index)
    {
        if (index < 0 || index >= Count) return null;
        if (modernImages != null)
        {
            using var reader = new BinaryReader(File.OpenRead(Path));
            return DecodeModern(reader, index, offsets[index]);
        }
        using var r = new BinaryReader(File.OpenRead(Path)); r.BaseStream.Position = offsets[index];
        int w = r.ReadInt16(), h = r.ReadInt16(), x = r.ReadInt16(), y = r.ReadInt16(); r.ReadInt16(); r.ReadInt16(); r.ReadByte(); int length = r.ReadInt32();
        if (w <= 0 || h <= 0) return null;
        if (w > 8192 || h > 8192 || length < 0 || length > r.BaseStream.Length - r.BaseStream.Position) throw new InvalidDataException("Invalid sprite dimensions or length.");
        byte[] raw = new byte[checked(w * h * 4)]; using (var gzip = new GZipStream(new MemoryStream(r.ReadBytes(length)), CompressionMode.Decompress)) { gzip.ReadExactly(raw); if (gzip.ReadByte() != -1) throw new InvalidDataException("Unexpected sprite pixel length."); }
        var bitmap = RasterImage.Create(w, h, 96, 96, RasterFormats.Bgra32, null, raw, w * 4);  return new(bitmap, x, y);
    }
    public Sprite? DecodeOverview(BinaryReader reader, int index, int level, CancellationToken cancellation = default)
    {
        cancellation.ThrowIfCancellationRequested();
        if (index < 0 || index >= Count) return null;
        if (modernImages != null) return DecodeModernOverview(reader, index, offsets[index], level, cancellation);
        reader.BaseStream.Position = offsets[index];
        int w = reader.ReadInt16(), h = reader.ReadInt16(), x = reader.ReadInt16(), y = reader.ReadInt16();
        reader.ReadInt16(); reader.ReadInt16(); reader.ReadByte(); int length = reader.ReadInt32();
        if (w <= 0 || h <= 0) return null;
        if (w > 8192 || h > 8192 || length < 0 || length > reader.BaseStream.Length - reader.BaseStream.Position)
            throw new InvalidDataException("Invalid sprite dimensions or length.");
        int width = Math.Max(1, (w + (1 << level) - 1) >> level), height = Math.Max(1, (h + (1 << level) - 1) >> level);
        byte[] compressed = ArrayPool<byte>.Shared.Rent(Math.Max(1, length));
        byte[] raw = ArrayPool<byte>.Shared.Rent(checked(w * h * 4));
        try
        {
            reader.BaseStream.ReadExactly(compressed.AsSpan(0, length));
            using (var gzip = new GZipStream(new MemoryStream(compressed, 0, length, false), CompressionMode.Decompress))
            {
                int pixelBytes = w * h * 4;
                for (int offset = 0; offset < pixelBytes; offset += 65536)
                {
                    cancellation.ThrowIfCancellationRequested();
                    gzip.ReadExactly(raw.AsSpan(offset, Math.Min(65536, pixelBytes - offset)));
                }
                if (gzip.ReadByte() != -1) throw new InvalidDataException("Unexpected sprite pixel length.");
            }
            byte[] pixels = OverviewPixels.Reduce(raw, w, h, width, height, cancellation);
            var bitmap = RasterImage.Create(width, height, 96, 96, RasterFormats.Bgra32, null, pixels, width * 4);
             return new(bitmap, x, y, w, h);
        }
        finally { ArrayPool<byte>.Shared.Return(compressed); ArrayPool<byte>.Shared.Return(raw); }
    }

    private byte[]? ReadModernBytes(BinaryReader reader, int index, long offset)
    {
        var image = modernImages![index];
        if (image.Length == 0 || offset < 0 || image.Length > reader.BaseStream.Length - offset) return null;
        reader.BaseStream.Position = offset;
        return reader.ReadBytes(image.Length);
    }

    private Sprite? DecodeModern(BinaryReader reader, int index, long offset)
    {
        var image = modernImages![index];
        var header = image.Header;
        if (ReadHeader(index) == null) return null;
        byte[]? compressed = ReadModernBytes(reader, index, offset);
        if (compressed == null) return null;
        if (image.FileType == 1)
            return new(RasterImage.Load(new MemoryStream(compressed)), header.X, header.Y);
        if (image.FileType != 0) throw new InvalidDataException($"Unsupported Sanxiu image type {image.FileType} at {Path}/{index}.");
        byte[] raw = new byte[checked(header.Width * header.Height * 4)];
        using (var gzip = new GZipStream(new MemoryStream(compressed), CompressionMode.Decompress))
        { gzip.ReadExactly(raw); if (gzip.ReadByte() != -1) throw new InvalidDataException("Unexpected sprite pixel length."); }
        return new(RasterImage.Create(header.Width, header.Height, 96, 96, RasterFormats.Bgra32, null, raw, header.Width * 4), header.X, header.Y);
    }

    private Sprite? DecodeModernOverview(BinaryReader reader, int index, long offset, int level, CancellationToken cancellation)
    {
        var image = modernImages![index];
        var header = image.Header;
        if (ReadHeader(index) == null) return null;
        byte[]? compressed = ReadModernBytes(reader, index, offset);
        if (compressed == null) return null;
        int width = Math.Max(1, (header.Width + (1 << level) - 1) >> level);
        int height = Math.Max(1, (header.Height + (1 << level) - 1) >> level);
        if (image.FileType == 1)
        {
            using var bitmap = RasterImage.Load(new MemoryStream(compressed));
            byte[] pixels = new byte[checked(bitmap.PixelWidth * bitmap.PixelHeight * 4)];
            bitmap.CopyPixels(pixels, bitmap.PixelWidth * 4, 0);
            width = Math.Max(1, (bitmap.PixelWidth + (1 << level) - 1) >> level);
            height = Math.Max(1, (bitmap.PixelHeight + (1 << level) - 1) >> level);
            return new(RasterImage.Create(width, height, 96, 96, bitmap.Format, null,
                OverviewPixels.Reduce(pixels, bitmap.PixelWidth, bitmap.PixelHeight, width, height, cancellation), width * 4),
                header.X, header.Y, bitmap.PixelWidth, bitmap.PixelHeight);
        }
        if (image.FileType != 0) throw new InvalidDataException($"Unsupported Sanxiu image type {image.FileType} at {Path}/{index}.");
        byte[] raw = ArrayPool<byte>.Shared.Rent(checked(header.Width * header.Height * 4));
        try
        {
            using var gzip = new GZipStream(new MemoryStream(compressed), CompressionMode.Decompress);
            int pixelBytes = header.Width * header.Height * 4;
            for (int start = 0; start < pixelBytes; start += 65536)
            {
                cancellation.ThrowIfCancellationRequested();
                gzip.ReadExactly(raw.AsSpan(start, Math.Min(65536, pixelBytes - start)));
            }
            if (gzip.ReadByte() != -1) throw new InvalidDataException("Unexpected sprite pixel length.");
            byte[] pixels = OverviewPixels.Reduce(raw, header.Width, header.Height, width, height, cancellation);
            return new(RasterImage.Create(width, height, 96, 96, RasterFormats.Bgra32, null, pixels, width * 4),
                header.X, header.Y, header.Width, header.Height);
        }
        finally { ArrayPool<byte>.Shared.Return(raw); }
    }
}
public sealed class AssetStore : IDisposable
{
    public Dictionary<int, IAssetLibrary> Libraries { get; } = [];
    public AssetSourceFormat Format { get; }
    private readonly ConcurrentDictionary<(int, int), Sprite> cache = [];
    private readonly ConcurrentDictionary<(int, int), byte> pending = [];
    private readonly ConcurrentDictionary<(int, int, int), Sprite> overviewCache = [];
    private readonly ConcurrentQueue<(int, int, int)> overviewOrder = [];
    private readonly ConcurrentDictionary<(int, int), byte> failed = [];
    private readonly ConcurrentQueue<(int, int)> order = [];
    private readonly Channel<(int, int)> queue = Channel.CreateBounded<(int, int)>(new BoundedChannelOptions(4096) { FullMode = BoundedChannelFullMode.Wait });
    private readonly CancellationTokenSource stop = new();
    private readonly CancellationToken stopToken;
    private readonly Task[] workers;
    private int disposed;
    private long bytes;
    private long overviewBytes;
    public event Action? Updated;
    public event Action<string>? Error;
    public int Cached => cache.Count;
    public long OverviewBytes => Interlocked.Read(ref overviewBytes);
    public int OverviewCached => overviewCache.Count;
    public bool IsResolved(int library, int index) => Volatile.Read(ref disposed) != 0 || cache.ContainsKey((library, index)) || failed.ContainsKey((library, index)) || !Libraries.TryGetValue(library, out var atlas) || index < 0 || index >= atlas.Count;
    public AssetStore(AssetSourceFormat format = AssetSourceFormat.Default)
    {
        Format = format;
        stopToken = stop.Token;
        workers = [Task.Run(Worker), Task.Run(Worker)];
    }
    public Sprite? Get(int lib, int image)
    {
        if (Volatile.Read(ref disposed) != 0) return null;
        var key = (lib, image); if (cache.TryGetValue(key, out var s)) return s;
        if (!Libraries.TryGetValue(lib, out var atlas) || image < 0 || image >= atlas.Count || failed.ContainsKey(key)) return null;
        if (pending.TryAdd(key, 0) && !queue.Writer.TryWrite(key)) pending.TryRemove(key, out _); return null;
    }

    public async Task<Dictionary<(int Library, int Image), Sprite>> PrepareOverviewAsync(
        IEnumerable<(int Library, int Image)> images, int level, CancellationToken cancellationToken)
    {
        using var cancellation = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken, stopToken);
        cancellation.Token.ThrowIfCancellationRequested();
        level = Math.Clamp(level, 0, 16);
        var result = new ConcurrentDictionary<(int Library, int Image), Sprite>();
        var missing = new List<(int Library, int Image)>();
        foreach (var key in images.Distinct())
        {
            cancellation.Token.ThrowIfCancellationRequested();
            if (failed.ContainsKey(key) || !Libraries.TryGetValue(key.Library, out var atlas) || key.Image < 0 || key.Image >= atlas.Count) continue;
            if (overviewCache.TryGetValue((key.Library, key.Image, level), out var cached)) result[key] = cached;
            else missing.Add(key);
        }
        var jobs = missing
            .GroupBy(k => k.Library).SelectMany(group => group.OrderBy(k => k.Image).Chunk(128)).ToArray();
        await Parallel.ForEachAsync(jobs, new ParallelOptions
        {
            MaxDegreeOfParallelism = Math.Clamp(Environment.ProcessorCount / 2, 2, 8),
            CancellationToken = cancellation.Token
        }, (job, token) =>
        {
            var library = Libraries[job[0].Library];
            using var reader = new BinaryReader(new FileStream(library.Path, FileMode.Open, FileAccess.Read, FileShare.Read, 65536));
            foreach (var key in job)
            {
                if (token.IsCancellationRequested) break;
                if (failed.ContainsKey(key)) continue;
                if (!overviewCache.TryGetValue((key.Library, key.Image, level), out var sprite))
                {
                    try { sprite = library.DecodeOverview(reader, key.Image, level, token); }
                    // Handle cooperative cancellation before it crosses the framework's
                    // synchronous worker callback (which can trigger user-unhandled breaks).
                    // Parallel.ForEachAsync still completes as canceled via its linked token.
                    catch (OperationCanceledException) when (token.IsCancellationRequested)
                    { return ValueTask.CompletedTask; }
                    catch (Exception ex) when (ex is IOException or InvalidDataException or ArgumentException)
                    { failed[key] = 0; StudioLog.Error("Assets.Overview", ex, $"Library={key.Library}; Image={key.Image}; File={library.Path}"); Error?.Invoke(ex.Message); continue; }
                    if (sprite == null) { failed[key] = 0; continue; }
                    if (token.IsCancellationRequested) { sprite.Bitmap.Dispose(); break; }
                    if (overviewCache.TryAdd((key.Library, key.Image, level), sprite))
                    {
                        overviewOrder.Enqueue((key.Library, key.Image, level));
                        Interlocked.Add(ref overviewBytes, (long)sprite.Bitmap.PixelWidth * sprite.Bitmap.PixelHeight * 4);
                        // Evict as we populate: cancellation must not bypass the cache budget.
                        while (Interlocked.Read(ref overviewBytes) > 64L * 1024 * 1024 && overviewOrder.TryDequeue(out var old))
                            if (overviewCache.TryRemove(old, out var removed)) Interlocked.Add(ref overviewBytes, -(long)removed.Bitmap.PixelWidth * removed.Bitmap.PixelHeight * 4);
                    }
                }
                result[key] = sprite;
            }
            return ValueTask.CompletedTask;
        });
        return result.ToDictionary(k => k.Key, k => k.Value);
    }

    private async Task Worker()
    {
        try
        {
            await foreach (var k in queue.Reader.ReadAllAsync(stopToken))
            {
                if (stopToken.IsCancellationRequested) break;
                try
                {
                    if (Libraries.TryGetValue(k.Item1, out var l))
                    {
                        var imageKey = (k.Item1, k.Item2);
                        var s = l.Decode(k.Item2);
                        if (stopToken.IsCancellationRequested) break;
                        if (s != null)
                        {
                            if (cache.TryAdd(imageKey, s))
                            {
                                order.Enqueue(imageKey); Interlocked.Add(ref bytes, (long)s.Bitmap.PixelWidth * s.Bitmap.PixelHeight * 4);
                                while (Interlocked.Read(ref bytes) > 384L * 1024 * 1024 && order.TryDequeue(out var old)) if (cache.TryRemove(old, out var removed)) Interlocked.Add(ref bytes, -(long)removed.Bitmap.PixelWidth * removed.Bitmap.PixelHeight * 4);
                            }
                        }
                        else failed[(k.Item1, k.Item2)] = 0;
                    }
                }
                catch (Exception ex) { failed[(k.Item1, k.Item2)] = 0; StudioLog.Error("Assets.Decode", ex, $"Library={k.Item1}; Image={k.Item2}; File={Libraries.GetValueOrDefault(k.Item1)?.Path}"); Error?.Invoke(ex.Message); }
                finally { pending.TryRemove(k, out _); Updated?.Invoke(); }
            }
        }
        catch (OperationCanceledException) { }
    }
    public void Dispose()
    {
        if (Interlocked.Exchange(ref disposed, 1) != 0) return;
        Updated = null; Error = null;
        stop.Cancel(); queue.Writer.TryComplete();
        _ = Task.WhenAll(workers).ContinueWith(completed =>
        {
            _ = completed.Exception;
            pending.Clear();
            stop.Dispose();
        }, CancellationToken.None, TaskContinuationOptions.ExecuteSynchronously, TaskScheduler.Default);
    }
    public static AssetStore Load(string root, AssetSourceFormat format = AssetSourceFormat.Default, string? attachmentsRoot = null)
    {
        var store = new AssetStore(format);
        try
        {
            store.Register(root);
            AttachedAssets.Register(store, attachmentsRoot ?? DataPaths.ResolveEditor("Attachments"));
            StudioLog.Info("Assets.Load", $"Root={root}; Format={format}; Libraries={store.Libraries.Count}");
            return store;
        }
        catch (Exception ex) { StudioLog.Error("Assets.Load", ex, $"Root={root}; Format={format}"); store.Dispose(); throw; }
    }
    private void Add(int id, string root, string relative) { string path = System.IO.Path.Combine(root, relative.Replace('\\', System.IO.Path.DirectorySeparatorChar) + ".lib"); if (File.Exists(path)) Libraries[id] = new CrystalLibrary(path); }
    private void Register(string root)
    {
        root = DataPaths.ResolveLibraryFolder(root);
        if (Format == AssetSourceFormat.Sanxiu) { RegisterSanxiu(root); return; }
        if (Format == AssetSourceFormat.Shanda) { RegisterShanda(root); return; }
        // IDs follow the reference editor's Crystal library table.
        string map = Directory.Exists(System.IO.Path.Combine(root, "Map")) ? System.IO.Path.Combine(root, "Map") : root;
        Add(0, map, @"WemadeMir2\Tiles"); Add(1, map, @"WemadeMir2\Smtiles"); Add(2, map, @"WemadeMir2\Objects");
        for (int i = 2; i < 99; i++) Add(i + 1, map, @"WemadeMir2\Objects" + i);
        Add(100, map, @"ShandaMir2\Tiles"); Add(110, map, @"ShandaMir2\SmTiles"); Add(120, map, @"ShandaMir2\Objects");
        for (int i = 1; i < 10; i++) { Add(100 + i, map, @"ShandaMir2\Tiles" + (i + 1)); Add(110 + i, map, @"ShandaMir2\SmTiles" + (i + 1)); }
        for (int i = 1; i < 31; i++) Add(120 + i, map, @"ShandaMir2\Objects" + (i + 1)); Add(190, map, @"ShandaMir2\AniTiles1");
        string[] names = ["Tilesc", "Tiles30c", "Tiles5c", "Smtilesc", "Housesc", "Cliffsc", "Dungeonsc", "Innersc", "Furnituresc", "Wallsc", "smObjectsc", "Animationsc", "Object1c", "Object2c"];
        string[] states = ["", "wood", "sand", "snow", "forest"];
        for (int i = 0; i < states.Length; i++) for (int j = 0; j < names.Length; j++)
        {
            Add(200 + i * 15 + j, map, System.IO.Path.Combine("WemadeMir3", states[i], names[j]));
            Add(300 + i * 15 + j, map, System.IO.Path.Combine("ShandaMir3", names[j] + states[i]));
        }
        // Flat folders are accepted for the original Mir2 libraries too.
        if (Libraries.Count == 0) { Add(0, map, "Tiles"); Add(1, map, "SmTiles"); Add(2, map, "Objects"); for (int i = 2; i < 99; i++) Add(i + 1, map, "Objects" + i); }
        if (Libraries.Count == 0) throw new InvalidDataException("No Crystal .lib files found. Select Data, Data/Map, or a flat Mir2 library folder.");
    }

    private void RegisterSanxiu(string root)
    {
        string map = Directory.Exists(System.IO.Path.Combine(root, "Map")) ? System.IO.Path.Combine(root, "Map") :
            Directory.Exists(System.IO.Path.Combine(root, "Data", "Map")) ? System.IO.Path.Combine(root, "Data", "Map") : root;
        foreach (string path in Directory.EnumerateFiles(map))
        {
            string file = System.IO.Path.GetFileName(path);
            if (!file.EndsWith(".lib", StringComparison.OrdinalIgnoreCase)) continue;
            string name = System.IO.Path.GetFileNameWithoutExtension(file);
            int id;
            if (TryMapId(name, "Tiles", 0, out id) ||
                TryMapId(name, "SmTiles", 1000, out id) ||
                TryMapId(name, "Objects", 2000, out id)) Libraries[id] = new CrystalLibrary(path, allowSanxiu: true);
        }
        if (Libraries.Count == 0)
            throw new InvalidDataException("No Sanxiu map libraries found. Select the Data folder or its Map folder.");
    }

    // Registers Shanda/Wemade .wzl/.wzx and .wil/.wix libraries. The same artwork is registered
    // under both the Wemade ID family (0/1/2...) and the Shanda 2012 family (100/110/120...) so
    // that old Type-0 maps and newer Shanda maps both resolve to the right sprites.
    private void RegisterShanda(string root)
    {
        var candidates = new List<string> { root };
        string? map = DataPaths.FindMapFolder(root);
        if (map != null)
        {
            candidates.Add(map);
            candidates.Add(System.IO.Path.Combine(map, "ShandaMir2"));
            candidates.Add(System.IO.Path.Combine(map, "WemadeMir2"));
        }
        candidates.Add(System.IO.Path.Combine(root, "ShandaMir2"));
        candidates.Add(System.IO.Path.Combine(root, "WemadeMir2"));
        candidates.Add(System.IO.Path.GetFullPath(System.IO.Path.Combine(root, "..")));
        var visited = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (var directory in candidates)
        {
            if (!Directory.Exists(directory) || !visited.Add(System.IO.Path.GetFullPath(directory))) continue;
            foreach (string indexFile in Directory.EnumerateFiles(directory))
            {
                string ext = System.IO.Path.GetExtension(indexFile);
                if (ext is not (".wzx" or ".wix")) continue;
                string name = System.IO.Path.GetFileNameWithoutExtension(indexFile);
                // Known named libraries (Tiles/Objects/...): register under both Wemade and Shanda ID families.
                if (FlatClientLibraries.TryMapIds(name, out int wemadeId, out int shandaId))
                {
                    IAssetLibrary? library = AssetLibraryFactory.TryCreate(System.IO.Path.Combine(directory, name));
                    if (library == null) continue;
                    if (wemadeId >= 0) Libraries[wemadeId] = library;
                    if (shandaId >= 0) Libraries[shandaId] = library;
                    continue;
                }
                // Numeric libraries: Shanda/Wemade clients name libraries 0.wzl..255.wzl (a byte index).
                // Register such files directly under their numeric library ID so all 256 slots resolve.
                if (int.TryParse(name, out int numericId) && numericId is >= 0 and <= 255)
                {
                    IAssetLibrary? library = AssetLibraryFactory.TryCreate(System.IO.Path.Combine(directory, name));
                    if (library == null) continue;
                    Libraries[numericId] = library;
                }
            }
        }
        if (Libraries.Count == 0)
            throw new InvalidDataException("No Shanda (.wzl/.wzx/.wil/.wix) libraries found. Select the Data folder or its Map folder.");
    }

    private static bool TryMapId(string name, string prefix, int baseId, out int id)
    {
        id = -1;
        if (name.Equals(prefix, StringComparison.OrdinalIgnoreCase)) { id = baseId; return true; }
        if (!name.StartsWith(prefix, StringComparison.OrdinalIgnoreCase) ||
            !int.TryParse(name.AsSpan(prefix.Length), out int number) || number is < 2 or > 1000) return false;
        id = baseId + number - 1;
        return true;
    }
}



