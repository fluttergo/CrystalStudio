using System.IO;
using System.Reflection;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Headless.XUnit;
using Avalonia.Interactivity;
using Avalonia.Threading;
using Avalonia.VisualTree;

namespace CrystalStudio.Tests;

public sealed class EditorReliabilityTests
{
    [Theory]
    [InlineData(4000, 4000, true)]
    [InlineData(4096, 3906, true)]
    [InlineData(4096, 3907, false)]
    [InlineData(4096, 4096, false)]
    [InlineData(15, 100, false)]
    [InlineData(4097, 100, false)]
    public void NewMapSizeMustAlsoBeReadable(int width, int height, bool allowed)
    {
        Assert.Equal(allowed, (MapDimensions.CreationError(width, height, MapCreationKind.Blank) == null));
        if (allowed) Assert.True(MapDimensions.CanRoundTrip(width, height));
    }

    [Fact]
    public void InvalidDimensionsCannotOverwriteAnExistingMap()
    {
        string path = TemporaryFile();
        try
        {
            File.WriteAllText(path, "original");
            Assert.Throws<ArgumentOutOfRangeException>(() => new MapDocument(4096, 4096).Save(path));
            Assert.Equal("original", File.ReadAllText(path));
            using (var stream = File.Create(path))
            using (var writer = new BinaryWriter(stream))
            {
                writer.Write((short)1); writer.Write((byte)'C'); writer.Write((byte)'#');
                writer.Write((short)4096); writer.Write((short)4096);
            }
            Assert.Throws<InvalidDataException>(() => MapDocument.Load(path));
        }
        finally { File.Delete(path); }
    }

    [AvaloniaFact]
    public void SelectiveMetadataPreservesMixedDoorsAndSupportsUndo()
    {
        var document = new MapDocument(4, 4);
        var first = new Cell { Light = 1, Door = 2, DoorOffset = 3, Foreground = new(2, 4) };
        var second = new Cell { Light = 2, Door = 5, DoorOffset = 6, BackImage = 17 };
        document.Set(1, 1, first); document.Set(2, 1, second);
        var viewport = new MapViewport(); viewport.SetDocument(document);
        var area = new Rect(1, 1, 2, 1);
        Assert.Null(viewport.CommonMetadata(area, c => c.Light));
        Assert.Null(viewport.CommonMetadata(area, c => c.Door));
        viewport.ApplyMetadata(null, null, null, area);
        Assert.False(viewport.History.Dirty);
        viewport.ApplyMetadata(100, null, null, area);
        Assert.Equal(first with { Light = 100 }, document.Get(1, 1));
        Assert.Equal(second with { Light = 100 }, document.Get(2, 1));
        Assert.Equal((byte)100, viewport.CommonMetadata(area, c => c.Light));
        viewport.Undo();
        Assert.Equal(first, document.Get(1, 1)); Assert.Equal(second, document.Get(2, 1));
        viewport.Redo();
        viewport.ApplyMetadata(null, 0, null, area);
        Assert.Equal(0, document.Get(1, 1).Door); Assert.Equal(0, document.Get(2, 1).Door);
        Assert.Equal(3, document.Get(1, 1).DoorOffset); Assert.Equal(6, document.Get(2, 1).DoorOffset);
        Assert.True(document.Get(0, 0).Empty);
    }

    [Fact]
    public void OverwritingMapAndStampPreservesThePreviousCompleteVersion()
    {
        string path = TemporaryFile(), stamp = TemporaryFile();
        try
        {
            var document = new MapDocument(2, 2);
            document.Set(0, 0, new Cell { Light = 100 }); document.Save(path);
            document.Set(0, 0, new Cell { Light = 119 }); document.Save(path);
            Assert.Equal(100, MapDocument.Load(path + ".bak").Get(0, 0).Light);
            Assert.Equal(119, MapDocument.Load(path).Get(0, 0).Light);
            document.Set(0, 0, new Cell { Light = 3 }); document.Save(path);
            Assert.Equal(119, MapDocument.Load(path + ".bak").Get(0, 0).Light);
            CellCodec.SaveStamp(stamp, [new(0, 0, new Cell { Light = 100 })]);
            CellCodec.SaveStamp(stamp, [new(0, 0, new Cell { Light = 119 })]);
            Assert.Equal(100, CellCodec.LoadStamp(stamp + ".bak")[0].Cell.Light);
            Assert.Throws<IOException>(() => AtomicFile.Write(path, stream => throw new IOException("write failed"), backup: true));
            Assert.Equal(3, MapDocument.Load(path).Get(0, 0).Light);
            Assert.Equal(119, MapDocument.Load(path + ".bak").Get(0, 0).Light);
        }
        finally { foreach (string file in new[] { path, path + ".bak", stamp, stamp + ".bak" }) File.Delete(file); }
    }

    [Fact]
    public void RecoveryRetainsUnsavedDataAndStampsButExcludesLiveSessions()
    {
        string directory = TemporaryFile();
        try
        {
            var cells = new List<StampCell> { new(0, 0, new Cell { Foreground = new(2002, 1), Light = 119, Door = 1 }) };
            var document = new MapDocument(4, 4);
            document.PlaceStamp(cells, 1, 1, "Test", new EditHistory(), (_, _) => { });
            var data = new MapRecoveryData(Guid.NewGuid(), "Unsaved", null, DateTime.UtcNow, 4, 4, AssetSourceFormat.Sanxiu,
                document.Cells.Select(p => new StampCell(p.Key % 4, p.Key / 4, p.Value.Copy())).ToList(), document.PlacedStamps.ToList(), true, -2, -2);
            using var reader = new MapRecoveryStore(directory);
            var writer = new MapRecoveryStore(directory);
            document.SourceFormat = AssetSourceFormat.Sanxiu;
            var metadata = new MapRecoveryEntry(1, writer.SessionId, data.Id, "", null, data.Name, data.SavedUtc,
                data.Format, null, IsStamp: true, OriginX: -2, OriginY: -2);
            writer.Write(data.Id, MapRecoveryStore.Capture(document), null, data.Name, null, metadata, writer.Version(data.Id));
            Assert.Empty(reader.List());
            writer.Dispose(); // Simulate process termination without normal cleanup.
            var entry = Assert.Single(reader.List());
            var restored = reader.Load(entry);
            Assert.Equal(document.Get(1, 1), restored.Get(1, 1));
            Assert.Equal(document.PlacedStamps[0].Id, Assert.Single(restored.PlacedStamps).Id);
            Assert.Equal(AssetSourceFormat.Sanxiu, restored.SourceFormat);
            Assert.True(entry.IsStamp); Assert.Equal(-2, entry.OriginX);
            reader.Delete(entry);
            Assert.Empty(reader.List());
        }
        finally { if (Directory.Exists(directory)) Directory.Delete(directory, true); }
    }

    [Fact]
    public void PreviousJsonRecoverySnapshotsRemainReadable()
    {
        string directory = TemporaryFile();
        try
        {
            string session = Path.Combine(directory, Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(session);
            var data = new MapRecoveryData(Guid.NewGuid(), "Legacy stamp", "original.X", DateTime.UtcNow,
                2, 2, AssetSourceFormat.Sanxiu, [new(1, 1, new Cell { Light = 119 })], [], true, -2, -4);
            AtomicFile.WriteJson(Path.Combine(session, data.Id.ToString("N") + ".json"), data);
            using var reader = new MapRecoveryStore(directory);
            var entry = Assert.Single(reader.List());
            Assert.True(entry.LegacyJson); Assert.True(entry.IsStamp);
            Assert.Equal(-2, entry.OriginX); Assert.Equal(-4, entry.OriginY);
            Assert.Equal("original.X", entry.OriginalPath);
            var restored = reader.Load(entry);
            Assert.Equal(119, restored.Get(1, 1).Light);
            Assert.Equal(AssetSourceFormat.Sanxiu, restored.SourceFormat);
            reader.Delete(entry); Assert.Empty(reader.List());
        }
        finally { if (Directory.Exists(directory)) Directory.Delete(directory, true); }
    }

    [Fact]
    public void SavingOrDiscardingInvalidatesPendingRecoveryWrites()
    {
        string directory = TemporaryFile();
        try
        {
            using var reader = new MapRecoveryStore(directory);
            var writer = new MapRecoveryStore(directory);
            var data = new MapRecoveryData(Guid.NewGuid(), "Test", "original.map", DateTime.UtcNow, 2, 2, AssetSourceFormat.Default, [], []);
            int version = writer.Version(data.Id);
            writer.Delete(data.Id);
            writer.Write(data.Id, new MapDocument(2, 2), data.OriginalPath, data.Name, null, version: version);
            writer.Dispose();
            Assert.Empty(reader.List());
            using (var clean = new MapRecoveryStore(directory))
            {
                clean.Write(data.Id, new MapDocument(2, 2), data.OriginalPath, data.Name, null);
                clean.ClearSession();
            }
            Assert.Empty(reader.List());
            Assert.Throws<InvalidDataException>(() => (data with { Width = 4096, Height = 4096 }).Restore());
            Assert.Throws<InvalidDataException>(() => (data with { Cells = [new(9, 0, new Cell())] }).Restore());
        }
        finally { if (Directory.Exists(directory)) Directory.Delete(directory, true); }
    }

    [Fact]
    public void RecoveredHistoryRequiresSavingWithoutAnArtificialUndoStep()
    {
        var document = new MapDocument(2, 2);
        document.Set(0, 0, new Cell { Light = 119 });
        var history = new EditHistory(); history.MarkUnsaved();
        Assert.True(history.Dirty); Assert.False(history.CanUndo);
        history.Undo(document);
        Assert.True(history.Dirty); Assert.Equal(119, document.Get(0, 0).Light);
        history.Record(document, 0, 0); document.Set(0, 0, new Cell { Light = 100 }); history.Commit();
        history.Undo(document);
        Assert.True(history.Dirty); Assert.Equal(119, document.Get(0, 0).Light);
        history.Saved(); Assert.False(history.Dirty);
    }

    [AvaloniaFact]
    public void MapCheckReportsBadReferencesDoorsAndDisconnectedAreas()
    {
        var document = new MapDocument(5, 3);
        for (int y = 0; y < 3; y++) document.Set(2, y, new Cell { BackImage = 0x20000000 });
        document.Set(0, 0, new Cell { BackLibrary = 777, BackImage = 1 });
        document.Set(4, 0, new Cell { MiddleLibrary = 1, MiddleImage = 2 });
        document.Set(0, 1, new Cell { Door = 1 });
        string libraryRoot = TemporaryFile();
        Fixture.CreateLibraries(libraryRoot);
        using var assets = AssetStore.Load(libraryRoot);
        var result = MapCheck.Run(document, assets.Libraries);
        Assert.Contains(result.Issues, i => i.X == 0 && i.Y == 0 && i.Message.Contains("777"));
        Assert.Contains(result.Issues, i => i.X == 4 && i.Y == 0 && i.Message.Contains("Image 2"));
        Assert.Contains(result.Issues, i => i.X == 0 && i.Y == 1 && i.Message.Contains("foreground"));
        Assert.Contains(result.Issues, i => i.Message.Contains("Disconnected"));
        Assert.Equal(result.Issues.Count, result.Total);
        using var cancellation = new CancellationTokenSource(); cancellation.Cancel();
        Assert.Throws<OperationCanceledException>(() => MapCheck.Run(document, null, cancellation.Token));
        assets.Dispose(); Directory.Delete(libraryRoot, true);
    }

    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public void MapCheckCapsResultsAndAcceptsConnectedOrBlockedMaps(bool blocked)
    {
        var document = new MapDocument(50, 50);
        for (int y = 0; y < 50; y++) for (int x = 0; x < 50; x++)
            document.Set(x, y, new Cell { BackImage = (blocked ? 0x60000000 : 0) | 1, BackLibrary = 777 });
        Assert.Empty(MapCheck.Run(document, null).Issues);
        var result = MapCheck.Run(document, new Dictionary<int, IAssetLibrary>());
        Assert.Equal(2500, result.Total); Assert.Equal(1000, result.Issues.Count);
    }

    [AvaloniaFact]
    public void RightClickTileLookupReturnsGroundAndMiddleSourceAssets()
    {
        var document = new MapDocument(4, 4);
        document.Set(0, 0, new Cell { BackLibrary = 7, BackImage = unchecked((int)0xA0000006), MiddleLibrary = 8, MiddleImage = 10 });
        var viewport = new MapViewport { Layer = MapLayer.Ground };
        viewport.SetDocument(document);
        var picked = new List<(int Library, int Index, MapLayer Layer)>();
        viewport.AssetPicked += (library, index, layer) => picked.Add((library, index, layer));

        Assert.True(viewport.PickTile(0, 0));
        Assert.Equal((7, 5, MapLayer.Ground), Assert.Single(picked));
        Assert.Equal(7, viewport.Library);
        Assert.Equal(5, viewport.SpriteIndex);

        viewport.Layer = MapLayer.Middle;
        Assert.True(viewport.PickTile(0, 0));
        Assert.Equal((8, 9, MapLayer.Middle), picked[1]);
        Assert.False(viewport.PickTile(3, 3));
    }

    [AvaloniaFact]
    public void LocksProtectPaintingMetadataAndWholeMapOperations()
    {
        var document = new MapDocument(4, 4);
        document.Set(0, 0, new Cell { BackImage = 1, MiddleImage = 2, Light = 119 });
        var viewport = new MapViewport { Layer = MapLayer.Ground, SpriteIndex = 2, Tool = EditorTool.Pencil };
        viewport.SetDocument(document);
        viewport.SetLayerLocked(MapLayer.Ground, true);
        Assert.Equal(0, viewport.BucketFill(0, 0));
        typeof(MapViewport).GetMethod("Apply", BindingFlags.NonPublic | BindingFlags.Instance)!.Invoke(viewport, [0, 0]);
        viewport.LocateCell(0, 0); viewport.ClearSelection(); viewport.FillSelection();
        Assert.Equal(1, document.Get(0, 0).BackImage); Assert.False(viewport.History.Dirty);
        viewport.Layer = MapLayer.Middle;
        Assert.True(viewport.BucketFill(0, 0) > 0);
        Assert.Equal(3, document.Get(0, 0).MiddleImage);
        viewport.SetLayerLocked(MapLayer.Objects, true);
        viewport.ApplyMetadata(0, 0, 0, new Rect(0, 0, 1, 1));
        Assert.Equal(119, document.Get(0, 0).Light);
        Assert.False(viewport.MovePickedTo(1, 1)); Assert.False(viewport.DeletePicked());
        Assert.False(viewport.ClonePickedTo(1, 1)); Assert.False(viewport.PaintWall(1, 1)); Assert.False(viewport.PlaceTownAt(1, 1));
        Assert.Throws<InvalidOperationException>(() => viewport.ApplyTemple(null!));
        Assert.Throws<InvalidOperationException>(() => viewport.ApplyProvince(null!));
        Assert.Throws<InvalidOperationException>(() => viewport.ApplyTerrain(null!));
        var state = viewport.CaptureEditorState();
        viewport.RestoreEditorState(new(new MapDocument(4, 4), new(), 1, default, null));
        Assert.False(viewport.AnyLayerLocked);
        viewport.RestoreEditorState(state);
        Assert.True(viewport.IsLayerLocked(MapLayer.Ground)); Assert.True(viewport.IsLayerLocked(MapLayer.Objects));
    }

    [AvaloniaFact]
    public void LocateHighlightsTheRequestedCellAndLocksDoNotChangeVisibility()
    {
        var viewport = new MapViewport();
        viewport.SetDocument(new MapDocument(10, 10));
        viewport.SetLayerLocked(MapLayer.Ground, true);
        Assert.True(viewport.VisibleLayers[0]);
        viewport.LocateCell(8, 7);
        Assert.Equal(new Rect(8, 7, 1, 1), viewport.Selection);
        Assert.True(viewport.Zoom >= .5);
        viewport.LocateCell(99, 99);
        Assert.Equal(new Rect(8, 7, 1, 1), viewport.Selection);
    }

    [AvaloniaFact]
    public void LockingAfterCopyPreventsCutAndPasteUntilUnlocked()
    {
        var document = new MapDocument(4, 4);
        document.Set(0, 0, new Cell { BackImage = 1, Foreground = new Front(2, 1) });
        var viewport = new MapViewport(); viewport.SetDocument(document);
        viewport.LocateCell(0, 0); viewport.CopySelection();
        Assert.True(viewport.CanPasteRegion);
        viewport.SetLayerLocked(MapLayer.Ground, true);
        viewport.LocateCell(0, 0); viewport.CutSelection();
        Assert.Equal(1, document.Get(0, 0).BackImage);
        typeof(MapViewport).GetField("hover", BindingFlags.NonPublic | BindingFlags.Instance)!.SetValue(viewport, (2, 2));
        var place = typeof(MapViewport).GetMethod("PlaceRegion", BindingFlags.NonPublic | BindingFlags.Instance)!;
        viewport.BeginPaste(); place.Invoke(viewport, null);
        Assert.True(document.Get(2, 2).Empty); Assert.False(viewport.History.Dirty);
        viewport.SetLayerLocked(MapLayer.Ground, false);
        viewport.BeginPaste(); place.Invoke(viewport, null);
        Assert.Equal(document.Get(0, 0), document.Get(2, 2));
        viewport.Undo(); Assert.True(document.Get(2, 2).Empty);
    }

    [AvaloniaFact]
    public async Task RecoveryTimerCapturesDirtyActiveAndInactiveTabs()
    {
        string directory = TemporaryFile();
        var window = new MainWindow();
        var field = typeof(MainWindow).GetField("recoveryStore", BindingFlags.NonPublic | BindingFlags.Instance)!;
        ((MapRecoveryStore?)field.GetValue(window))?.ClearSession();
        var store = new MapRecoveryStore(directory); field.SetValue(window, store);
        var viewport = window.FindControl<MapViewport>("Viewport")!;
        var tabs = (TabControl)typeof(MainWindow).GetField("mapTabs", BindingFlags.NonPublic | BindingFlags.Instance)!.GetValue(window)!;
        var add = typeof(MainWindow).GetMethod("AddMapTab", BindingFlags.NonPublic | BindingFlags.Instance)!;
        try
        {
            typeof(MainWindow).GetField("settings", BindingFlags.NonPublic | BindingFlags.Instance)!.SetValue(window, new LibrarySettings(AssetFormat: AssetSourceFormat.Sanxiu));
            add.Invoke(window, [new MapDocument(4, 4), null]);
            viewport.ApplyMetadata(100, null, null, new Rect(1, 1, 1, 1));
            add.Invoke(window, [new MapDocument(3, 3), null]);
            viewport.ApplyMetadata(119, null, null, new Rect(2, 2, 1, 1));
            await window.SaveRecoverySnapshotsAsync();
            using var reader = new MapRecoveryStore(directory);
            Assert.Empty(reader.List());
            store.Dispose();
            var entries = reader.List();
            Assert.Equal(2, entries.Count);
            Assert.All(entries, entry => Assert.Equal(AssetSourceFormat.Sanxiu, entry.AssetFormat));
            Assert.Equal(100, entries.Select(reader.Load).Single(d => d.Width == 4).Get(1, 1).Light);
            Assert.Equal(119, entries.Select(reader.Load).Single(d => d.Width == 3).Get(2, 2).Light);
        }
        finally
        {
            foreach (var tab in tabs.Items.Cast<TabItem>()) { tabs.SelectedItem = tab; viewport.History.Saved(); }
            window.Close(); store.Dispose();
            if (Directory.Exists(directory)) Directory.Delete(directory, true);
        }
    }

    [AvaloniaFact]
    public async Task PropertyDialogAndLayerLockControlsApplyToTheActualViewport()
    {
        var window = new MainWindow(); window.Show();
        var viewport = window.FindControl<MapViewport>("Viewport")!;
        try
        {
            await WaitForStartup(window);
            viewport.Document.Set(1, 1, new Cell { Light = 1, Door = 2, DoorOffset = 3 });
            viewport.Document.Set(2, 1, new Cell { Light = 2, Door = 5, DoorOffset = 6 });
            Exception? failure = null;
            Dispatcher.UIThread.Post(() =>
            {
                var dialog = window.OwnedWindows.OfType<EditorWindow>().Single();
                try
                {
                    var fields = dialog.GetVisualDescendants().OfType<TextBox>().ToArray();
                    Assert.Equal(3, fields.Length);
                    Assert.All(fields, box => Assert.True(string.IsNullOrEmpty(box.Text)));
                    fields[0].Text = "100";
                    dialog.GetVisualDescendants().OfType<Button>().Single(b => Equals(b.Content, LocalText.T("Apply Properties")))
                        .RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
                }
                catch (Exception ex) { failure = ex; }
                finally { if (dialog.IsVisible) dialog.Close(); }
            });
            typeof(MainWindow).GetMethod("OpenCellProperties", BindingFlags.NonPublic | BindingFlags.Instance)!
                .Invoke(window, [ValueTuple.Create(1, 1), new Rect(1, 1, 2, 1)]);
            Assert.Null(failure);
            Assert.Equal(100, viewport.Document.Get(2, 1).Light);
            Assert.Equal(5, viewport.Document.Get(2, 1).Door);
            Assert.Equal(6, viewport.Document.Get(2, 1).DoorOffset);
            var row = (ListBoxItem)window.FindControl<ListBox>("LayerList")!.Items[0]!;
            var toggle = ((Grid)row.Content!).Children.OfType<CheckBox>().Single(c => c.Name == "LayerLock0");
            toggle.IsChecked = true;
            Assert.True(viewport.IsLayerLocked(MapLayer.Ground));
            Assert.True(viewport.VisibleLayers[0]);
        }
        finally { viewport.History.Saved(); window.Close(); }
    }

    [AvaloniaFact]
    public async Task MapCheckDialogLocatesAReportedCell()
    {
        var window = new MainWindow(); window.Show();
        var viewport = window.FindControl<MapViewport>("Viewport")!;
        try
        {
            await WaitForStartup(window);
            viewport.SetDocument(new MapDocument(4, 4));
            viewport.Document.Set(2, 3, new Cell { BackLibrary = 777, BackImage = 1 });
            Exception? failure = null;
            Dispatcher.UIThread.Post(async () =>
            {
                var dialog = window.OwnedWindows.OfType<EditorWindow>().Single();
                try
                {
                    var list = dialog.GetVisualDescendants().OfType<ListBox>().Single();
                    for (int i = 0; i < 200 && list.Items.Count == 0; i++) await Task.Delay(10);
                    Assert.Single(list.Items);
                    dialog.GetVisualDescendants().OfType<Button>().Single(b => Equals(b.Content, LocalText.T("Locate")))
                        .RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
                }
                catch (Exception ex) { failure = ex; }
                finally { if (dialog.IsVisible) dialog.Close(); }
            });
            typeof(MainWindow).GetMethod("CheckMapClick", BindingFlags.NonPublic | BindingFlags.Instance)!
                .Invoke(window, [window, new RoutedEventArgs()]);
            Assert.Null(failure);
            Assert.Equal(new Rect(2, 3, 1, 1), viewport.Selection);
        }
        finally { viewport.History.Saved(); window.Close(); }
    }

    private static async Task WaitForStartup(MainWindow window)
    {
        var busy = typeof(MainWindow).GetField("busy", BindingFlags.NonPublic | BindingFlags.Instance)!;
        var ready = typeof(MainWindow).GetField("startupComplete", BindingFlags.NonPublic | BindingFlags.Instance)!;
        for (int i = 0; i < 500; i++)
        {
            if (Equals(ready.GetValue(window), true) && Equals(busy.GetValue(window), false)) return;
            await Task.Delay(10); Dispatcher.UIThread.RunJobs();
        }
        Assert.Fail("Editor startup did not finish.");
    }

    private static string TemporaryFile() => Path.Combine(Path.GetTempPath(), "CrystalStudio-Reliability-" + Guid.NewGuid().ToString("N"));
}
