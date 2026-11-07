using System.IO;

namespace CrystalStudio;

internal sealed record MapIssue(int X, int Y, string Message)
{
    public override string ToString() => $"({X}, {Y})  {Message}";
}
internal sealed record MapCheckResult(List<MapIssue> Issues, int Total);

internal static class MapCheck
{
    internal static MapCheckResult Run(MapDocument document, IReadOnlyDictionary<int, IAssetLibrary>? libraries,
        CancellationToken cancellation = default)
    {
        var issues = new List<MapIssue>();
        int total = 0;
        void Add(int x, int y, string message)
        {
            total++;
            if (issues.Count < 1000) issues.Add(new(x, y, message));
        }
        var checkedImages = new Dictionary<(int, int), string?>();
        void Image(int x, int y, int library, int image, string layer)
        {
            if (image == 0 || libraries == null) return;
            if (!checkedImages.TryGetValue((library, image), out string? error))
            {
                error = null;
                if (!libraries.TryGetValue(library, out var atlas)) error = "Missing library";
                else if (image < 1 || image > atlas.Count) error = "Invalid image number";
                else
                {
                    try { if (atlas.ReadHeader(image - 1) == null) error = "Empty image"; }
                    catch (Exception ex) when (ex is IOException or InvalidDataException or UnauthorizedAccessException)
                    { error = "Unreadable image"; }
                }
                checkedImages[(library, image)] = error;
            }
            if (error != null) Add(x, y, LocalText.Format("{0}: {1} · Library {2}, Image {3}", LocalText.T(layer), LocalText.T(error), library, image));
        }
        foreach (var (key, cell) in document.Cells)
        {
            cancellation.ThrowIfCancellationRequested();
            int x = key % document.Width, y = key / document.Width;
            Image(x, y, cell.BackLibrary, cell.BackImage & 0x1FFFFFFF, "Ground");
            Image(x, y, cell.MiddleLibrary, cell.MiddleImage, "Middle");
            if (cell.Foreground is { } front)
            {
                int image = front.Image & 0x7FFF;
                Image(x, y, front.Library, image, "Objects");
                if (image != 0 && (cell.Door & 0x7F) != 0) Image(x, y, front.Library, image + cell.DoorOffset, "Open door");
            }
            if (!MapPreview.HasDoor(cell)) continue;
            if (!cell.HasForeground) Add(x, y, LocalText.T("Door has no foreground image"));
            bool neighbour = false;
            for (int dy = -1; dy <= 1; dy++) for (int dx = -1; dx <= 1; dx++)
                if ((dx != 0 || dy != 0) && Walkable(document, x + dx, y + dy)) neighbour = true;
            if (!neighbour) Add(x, y, LocalText.T("Door has no walkable neighbouring cell"));
        }
        // Report disconnected components as warnings: separate islands may be intentional.
        var visited = new bool[checked(document.Width * document.Height)];
        var queue = new Queue<int>();
        (int First, int Size)? largest = null;
        void Disconnected(int first, int size) => Add(first % document.Width, first / document.Width,
            LocalText.Format("Disconnected walkable area: {0:N0} cells", size));
        for (int key = 0; key < visited.Length; key++)
        {
            if ((key & 4095) == 0) cancellation.ThrowIfCancellationRequested();
            if (visited[key]) continue;
            visited[key] = true;
            if (!Walkable(document, key % document.Width, key / document.Width)) continue;
            int size = 0;
            queue.Enqueue(key);
            while (queue.TryDequeue(out int next))
            {
                if ((size++ & 4095) == 0) cancellation.ThrowIfCancellationRequested();
                int x = next % document.Width, y = next / document.Width;
                for (int dy = -1; dy <= 1; dy++) for (int dx = -1; dx <= 1; dx++)
                {
                    int nx = x + dx, ny = y + dy;
                    if (!document.Contains(nx, ny)) continue;
                    int n = ny * document.Width + nx;
                    if (visited[n]) continue;
                    visited[n] = true;
                    if (Walkable(document, nx, ny)) queue.Enqueue(n);
                }
            }
            if (largest == null || size > largest.Value.Size)
            {
                if (largest is { } previous) Disconnected(previous.First, previous.Size);
                largest = (key, size);
            }
            else Disconnected(key, size);
        }
        return new(issues, total);
    }

    private static bool Walkable(MapDocument document, int x, int y) => document.Contains(x, y) &&
        (document.Get(x, y).BackImage & 0x60000000) == 0;
}
