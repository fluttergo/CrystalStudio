using System.Diagnostics;
namespace CrystalStudio;

public enum EditorTool { Brush, Pencil, Bucket, Eraser, Select, Stamp, Pickup, Clone, SpecialBrush, Wall, TownStamp, Eyedropper, Pan }
public sealed partial class MapViewport : SceneControl
{
    public MapDocument Document { get; private set; } = new(256, 256);
    public EditHistory History { get; private set; } = new();
    public AssetStore? Assets { get; set; }
    private TilePatternLayout BrushLayout => AutoTileBrush.Layout(Library, Layer, Assets?.Format ?? AssetSourceFormat.Default);
    private EditorTool tool;
    public EditorTool Tool
    {
        get => tool;
        set
        {
            if (tool == value) return;
            if (dragging) End();
            CancelWallDraft(); CancelPickup(); CancelPaste(); tool = value; Deselect();
        }
    }
    public MapLayer Layer { get; set; }
    public bool IsCollisionLayer => Layer is MapLayer.LowCollision or MapLayer.HighCollision;
    private int ActiveCollisionMask => Layer == MapLayer.LowCollision ? 0x20000000 : 0x40000000;
    public int Library { get; set; }
    public int SpriteIndex { get; set; } = -1;
    private int brushSize = 3;
    public int BrushSize { get => brushSize; set => brushSize = Math.Clamp(value | 1, 3, 31); }
    public Rect PaintFootprint(int x, int y)
    {
        int step = Tool != EditorTool.SpecialBrush && Layer == MapLayer.Ground ? 2 : 1;
        if (step == 2) { x &= ~1; y &= ~1; }
        int size = Tool is EditorTool.Pencil or EditorTool.Bucket ? 1 : BrushSize;
        int radius = size / 2;
        return new(x - radius * step, y - radius * step, size * step, size * step);
    }
    public List<StampCell>? Stamp { get; set; }
    public string StampName {get;set;}="Object stamp";
    public Guid? PickedStampId {get;private set;}
    public bool IsStampPickingTool => Tool is EditorTool.Pickup or EditorTool.Clone;
    private (int X,int Y) pickupOffset;
    public PlacedStamp? PickedStamp=>PickedStampId is Guid id ? Document.PlacedStamps.FirstOrDefault(s=>s.Id==id) : null;
    public event Action<string>? Message;
    public void CancelPickup(){PickedStampId=null;InvalidateVisual();}
    public bool PickupAt(int x,int y)
    {
        var stamp=Document.StampAt(x,y);if(stamp==null){Message?.Invoke(LocalText.T("No remembered stamp here \u00b7 Imported map objects need to be placed as stamps first"));return false;}
        if(stamp.Wall!=null){Message?.Invoke(LocalText.T("Connected wall \u00b7 Use Wall (W) to extend it or Shift to remove sections"));return false;}
        PickedStampId=stamp.Id;pickupOffset=(x-stamp.X,y-stamp.Y);InvalidateVisual();Changed?.Invoke();Message?.Invoke(((Tool==EditorTool.Clone ? (LocalText.Format("{0} · Click to clone · Escape to cancel", stamp.Name)) : (LocalText.Format("{0} · Click to move · Delete to remove · Escape to cancel", stamp.Name)))));return true;
    }
    public bool MovePickedTo(int x,int y)
    {
        if (!AllowEdit()) return false;
        if(PickedStampId is not Guid id)return false;
        if(PickedStamp is not PlacedStamp picked)return false;
        var target=PickedTarget(x,y);
        var placement=GetStampPreviewPlacement(picked.Cells,target.X,target.Y,id);
        if(!placement.Allowed){Message?.Invoke(LocalText.Placement(placement));InvalidateVisual();return false;}
        if(!Document.MoveStamp(id,target.X,target.Y,History,InvalidateCell)){Message?.Invoke(LocalText.T("Placement blocked \u00b7 Stamp data cannot be placed"));return false;}
        History.Commit();CancelPickup();Changed?.Invoke();Message?.Invoke(LocalText.T("Stamp moved"));return true;
    }
    public bool DeletePicked()
    {
        if (!AllowEdit()) return false;
        if(Tool==EditorTool.Clone)return false;
        if(PickedStampId is not Guid id)return false;
        bool deleted=Document.DeleteStamp(id,History,InvalidateCell);History.Commit();CancelPickup();Changed?.Invoke();Message?.Invoke(LocalText.T("Stamp deleted"));return deleted;
    }
    public bool ClonePickedTo(int x,int y)
    {
        if (!AllowEdit()) return false;
        if(PickedStamp is not PlacedStamp picked)return false;
        var target=PickedTarget(x,y);int targetX=target.X,targetY=target.Y;
        var placement=GetStampPreviewPlacement(picked.Cells,targetX,targetY);
        if(!placement.Allowed){Message?.Invoke(LocalText.Placement(placement));InvalidateVisual();return false;}
        if(Document.PlaceStamp(picked.Cells,targetX,targetY,picked.Name,History,InvalidateCell)==null){Message?.Invoke(LocalText.T("Placement blocked \u00b7 Stamp data cannot be placed"));return false;}
        History.Commit();CancelPickup();Changed?.Invoke();Message?.Invoke(LocalText.T("Stamp cloned"));return true;
    }
    /// <summary>
    /// Place a resolved .set composition (or any stamp cell list) at the map cell under a
    /// screen point. Used by drag-and-drop from the Composition Models panel so a composite
    /// model can be dropped directly onto the open map, exactly like a model-library stamp.
    /// </summary>
    public bool DropSetComposition(IReadOnlyList<StampCell> cells, string name, Point screenPoint)
    {
        if (cells == null || cells.Count == 0) return false;
        if (!AllowEdit()) return false;
        var (x, y) = CellAt(screenPoint);
        var placement = Document.CheckStampPlacement(cells, x, y);
        if (!placement.Allowed) { Message?.Invoke(LocalText.Placement(placement)); return false; }
        var placed = Document.PlaceStamp(cells, x, y, name, History, InvalidateCell);
        if (placed != null)
        {
            Changed?.Invoke();
            AssetUsed?.Invoke();
            Message?.Invoke(LocalText.Format("{0} placed", name));
        }
        else Message?.Invoke(LocalText.T("Placement blocked · Stamp data or instance limit prevents placement"));
        return placed != null;
    }
    public bool GridVisible { get; set; } = true;
    public bool LowCollisionVisible { get; set; }
    public bool HighCollisionVisible { get; set; }
    public bool CollisionVisible
    {
        get => LowCollisionVisible || HighCollisionVisible;
        set { LowCollisionVisible = value; HighCollisionVisible = value; }
    }
    private int VisibleCollisionMask => (LowCollisionVisible ? 0x20000000 : 0) | (HighCollisionVisible ? 0x40000000 : 0);
    private int RenderLayerSettings => (VisibleLayers[0] ? 1 : 0) | (VisibleLayers[1] ? 2 : 0) |
        (VisibleLayers[2] ? 4 : 0) | (LowCollisionVisible ? 8 : 0) | (HighCollisionVisible ? 32 : 0) | (DoorsOpen ? 16 : 0);
    public bool LightCellsVisible { get; set; }
    public bool DoorCellsVisible { get; set; }
    public MapLighting Lighting { get; set; } = MapLighting.Day;
    public bool DarknessVisible => Lighting != MapLighting.Day;
    private bool doorsOpen;
    public bool DoorsOpen
    {
        get => doorsOpen;
        set
        {
            if (doorsOpen == value) return;
            doorsOpen = value; frontRows.Clear(); sceneDirty = true; InvalidateVisual();
        }
    }
    public bool[] VisibleLayers { get; } = [true, true, true];
    public double Zoom { get; private set; } = 1;
    public Rect? Selection { get; private set; }
    public void Deselect()
    {
        if (dragging && !panning && Tool == EditorTool.Select) End();
        if (Selection == null) return;
        Selection = null; InvalidateVisual(); Changed?.Invoke();
    }
    public void CancelInteraction()
    {
        NavigationCancelled?.Invoke();
        if (dragging) End();
        CancelWallDraft(true); CancelPaste(); CancelPickup(); Deselect();
    }
    private bool SelectionContains(int x, int y) => Selection is Rect area && x >= area.Left && x < area.Right && y >= area.Top && y < area.Bottom;
    public event Action<int, int>? Hover;
    // Raised when the user right-clicks a ground or middle tile to select its source asset.
    public event Action<int, int, MapLayer>? AssetPicked;
    public event Action? Changed;
    public event Action? AssetUsed;
    public event Action<double, int>? Rendered;
    private Point origin = new(60, 70), mouse, panStart, originStart;
    private (int X, int Y) hover = (-1, -1), selectionStart, lastPaint = (-999, -999);
    private bool dragging, panning, selectionDragged;
    private readonly Dictionary<(int, int, int), SceneGroup> chunks = [];
    private int drawn;
    private int animationFrame;
    private readonly MapSpriteDrawing.AdditiveCache detailEffects=new();
    private int editRevision;
    private (int, int, int, int, int, MapLayer, int)? brushPreviewKey;
    private List<StampCell> brushPreview = [];
    private readonly HashSet<(int, int, int)> animatedChunks = [];
    private readonly HashSet<(int, int)> animatedRows = [];
    private readonly DispatcherTimer animationTimer = new() { Interval = TimeSpan.FromMilliseconds(100) };
    public (int X, int Y) HoverCell => hover;
    private readonly Dictionary<(int, int), SceneGroup> frontRows = [];
    private static Brush canvas => ThemeService.Brush("CanvasBackground");
    private static Brush paper => ThemeService.Brush("CanvasPaper");
    private static ScenePen cursor => new(ThemeService.Brush("Accent"),1.5);
    private static readonly ScenePen hoverCellOutline = new(new ImmutableSolidColorBrush(Colors.DarkBlue, .7), 4);
    public MapViewport() { InitializeScene(); Focusable = true; ClipToBounds = true; RenderOptions.SetBitmapInterpolationMode(this, Avalonia.Media.Imaging.BitmapInterpolationMode.None); animationTimer.Tick += (_, _) => AdvanceAnimation(); Loaded += (_, _) => { animationTimer.Start(); sceneDirty = true; InvalidateVisual(); }; Unloaded += (_, _) => { PauseOverview(); animationTimer.Stop(); NavigationCancelled?.Invoke(); if (dragging) End(); }; }
    public void SetDocument(MapDocument d, bool preserveZoom = false) { NavigationCancelled?.Invoke(); if (dragging) End(); ResetOverview(); ResetPresentation(); ResetDetailScene(); ResetPickupHints(); ResetStampDrawing(); CancelPickup(); CancelPaste(); CancelWallDraft(); Document = d; Array.Clear(layerLocks); History = new(); Selection = null; brushPreviewKey = null; chunks.Clear(); frontRows.Clear(); origin = new(60, 70); Zoom = preserveZoom ? LimitZoom(Zoom) : 1; WarmOverview(); InvalidateVisual(); Changed?.Invoke(); }
    internal void StopBackgroundWork() { ResetOverview(); animationTimer.Stop(); }
    internal MapEditorState CaptureEditorState()
    {
        NavigationCancelled?.Invoke();
        if (dragging) End();
        CancelWallDraft(true); CancelPaste(); CancelPickup();
        return new(Document, History, Zoom, origin, Selection, (bool[])layerLocks.Clone());
    }
    internal void RestoreEditorState(MapEditorState state)
    {
        SetDocument(state.Document);
        History = state.History; Zoom = state.Zoom; origin = state.Origin; Selection = state.Selection;
        for (int i = 0; i < layerLocks.Length; i++) layerLocks[i] = state.LayerLocks?[i] ?? false;
        hover = (-1, -1);
        MinimapInvalidated?.Invoke(); InvalidateVisual(); Changed?.Invoke();
    }
    public void RefreshAssets() { detailAssetRevision++; RefreshStampDrawing(); if (Zoom >= 0.25 || DarknessVisible) sceneDirty = true; RefreshOverviewAssets(); brushPreviewKey = null; chunks.Clear(); frontRows.Clear(); if (sceneDirty) InvalidateVisual(); else UpdateInteractionScene(); }
    public void InvalidateCell(int x, int y) { sceneDirty = true; InvalidateOverviewCell(x, y); editRevision++; for (int l = 0; l < 2; l++) chunks.Remove((x / 16, y / 16, l)); frontRows.Remove((x / 16, y)); InvalidateVisual(); }
    public void Undo() { if (movingSelection != null) { CancelInteraction(); return; } if (pendingWall.Count > 0) { CancelWallDraft(true); InvalidateVisual(); return; } CancelWallDraft(); CancelPickup(); brushPreviewKey = null; int width = Document.Width, height = Document.Height; History.Undo(Document, InvalidateCell); if (width != Document.Width || height != Document.Height) { RefreshAfterResize(); return; } chunks.Clear(); frontRows.Clear(); InvalidateVisual(); Changed?.Invoke(); }
    public void Redo() { if (movingSelection != null) { CancelInteraction(); return; } CancelWallDraft(); CancelPickup(); brushPreviewKey = null; int width = Document.Width, height = Document.Height; History.Redo(Document, InvalidateCell); if (width != Document.Width || height != Document.Height) { RefreshAfterResize(); return; } chunks.Clear(); frontRows.Clear(); InvalidateVisual(); Changed?.Invoke(); }
    private double FitZoom => Math.Min(1, Math.Min(Math.Max(1, ActualWidth - 80) / (Document.Width * 48.0), Math.Max(1, ActualHeight - 80) / (Document.Height * 32.0)));
    public void Fit() { Zoom = FitZoom; origin = new((ActualWidth - Document.Width * 48 * Zoom) / 2, (ActualHeight - Document.Height * 32 * Zoom) / 2); InvalidateVisual(); Changed?.Invoke(); }
    private double LimitZoom(double zoom) => Math.Clamp(double.IsFinite(zoom) && zoom > 0 ? zoom : Zoom, FitZoom, 1);
    protected override void OnSizeChanged(SizeChangedEventArgs sizeInfo)
    {
        base.OnSizeChanged(sizeInfo);
        InvalidateVisual();
        if (IsModulePreview) return;
        if (Zoom >= FitZoom) return;
        var worldCentre = (new Point(sizeInfo.PreviousSize.Width / 2, sizeInfo.PreviousSize.Height / 2) - origin) / Zoom;
        Zoom = FitZoom;
        origin = new Point(ActualWidth / 2, ActualHeight / 2) - worldCentre * Zoom;
        InvalidateVisual(); Changed?.Invoke();
    }
    public void SetZoom(double zoom)
    {
        var centre = new Point(ActualWidth / 2, ActualHeight / 2);
        var world = (centre - origin) / Zoom;
        Zoom = LimitZoom(zoom);
        origin = centre - world * Zoom;
        InvalidateVisual(); Changed?.Invoke();
    }
    private protected override void RenderScene(SceneContext dc)
    {
        dc.DrawRectangle(canvas, null, new(0, 0, ActualWidth, ActualHeight));
    }
    private bool sceneUpdatePending;
    private IBrush? renderedBackground;
    public new void InvalidateVisual()
    {
        if (sceneUpdatePending) return;
        sceneUpdatePending = true;
        Dispatcher.UIThread.Post(() =>
        {
            sceneUpdatePending = false;
            if (this.GetVisualRoot() == null) return;
            UpdateRetainedScenes();
            if (!ReferenceEquals(renderedBackground, canvas))
            {
                renderedBackground = canvas;
                base.InvalidateVisual();
            }
        }, DispatcherPriority.Render);
    }
    private void UpdateRetainedScenes()
    {
        var watch = Stopwatch.StartNew(); drawn = 0;
        int builds = sceneBuilds;
        UpdateMapScene();
        UpdateInteractionScene();
        if (sceneBuilds != builds) NotifyRendered(watch.Elapsed.TotalMilliseconds, drawn);
        if (chunks.Count > 2048 || frontRows.Count > 8192) { chunks.Clear(); frontRows.Clear(); }
    }
    private bool notificationPending;
    private (double Milliseconds, int Count) latestRender;
    private void NotifyRendered(double milliseconds, int count)
    {
        latestRender = (milliseconds, count);
        if (notificationPending) return;
        notificationPending = true;
        Dispatcher.UIThread.Post(() =>
        {
            notificationPending = false;
            if (!IsLoaded) return;
            MinimapInvalidated?.Invoke();
            Rendered?.Invoke(latestRender.Milliseconds, latestRender.Count);
        }, DispatcherPriority.Background);
    }
    private void DrawMapScene(SceneContext dc)
    {
        dc.PushTransform(new SceneTranslation(origin.X, origin.Y)); dc.PushTransform(new SceneScale(Zoom, Zoom));
        dc.DrawRectangle(paper, null, new(0, 0, Document.Width * 48, Document.Height * 32));
        int x0 = Math.Max(0, (int)Math.Floor(-origin.X / Zoom / 48) - 35), y0 = Math.Max(0, (int)Math.Floor(-origin.Y / Zoom / 32) - 2);
        int x1 = Math.Min(Document.Width - 1, (int)((ActualWidth - origin.X) / Zoom / 48) + 35), y1 = Math.Min(Document.Height - 1, (int)((ActualHeight - origin.Y) / Zoom / 32) + 35);
        // Retained chunk drawings avoid rebuilding thousands of sprite commands while panning.
        if (Zoom < 0.25) DrawOverview(dc, x0, y0, x1, y1);
        else if (x0 <= x1 && y0 <= y1)
        {
            if(ReuseDetailScene())
            {dc.DrawDrawing(detailScene);dc.Pop();dc.Pop();return;}
            x0=Math.Max(0,(int)Math.Floor(detailCoverage.Left/48)-35);
            y0=Math.Max(0,(int)Math.Floor(detailCoverage.Top/32)-2);
            x1=Math.Min(Document.Width-1,(int)Math.Ceiling(detailCoverage.Right/48)+35);
            y1=Math.Min(Document.Height-1,(int)Math.Ceiling(detailCoverage.Bottom/32)+35);
            var scene = new SceneGroup();
            var background = new MapSpriteDrawing.BackgroundIndex(scene);
            scene.Children.Add(new SceneRectangle(paper, null, new(0, 0, Document.Width * 48.0, Document.Height * 32.0)));
            for (int layer = 0; layer < 3; layer++) if (VisibleLayers[layer])
            {
                if (layer < 2) for (int cy = y0 / 16; cy <= y1 / 16; cy++) for (int cx = x0 / 16; cx <= x1 / 16; cx++)
                {
                    bool blend = layer == 1 && HasMiddleBlend(cx,cy);
                    if (!blend)
                    {
                        var chunk = Chunk(cx, cy, layer);
                        if (chunk.Children.Count != 0) scene.Children.Add(chunk);
                    }
                    else for (int y = cy * 16; y < Math.Min(Document.Height, cy * 16 + 16); y++) for (int x = cx * 16; x < Math.Min(Document.Width, cx * 16 + 16); x++)
                    {
                        var cell = Document.Get(x, y); if (cell.MiddleImage == 0 || cell.MiddleLibrary < 0) continue;
                        int frames = MapSpriteDrawing.MiddleFrames(cell.MiddleFrames);
                        if (frames > 1) animatedChunks.Add((cx, cy, layer));
                        AppendSprite(scene, background, x, y, cell.MiddleLibrary, cell.MiddleImage - 1 + (frames > 0 ? animationFrame / (1 + cell.MiddleTick) % frames : 0), true, false, MapSpriteDrawing.MiddleBlend(cell.MiddleFrames));
                    }
                }
                else for (int y = y0; y <= y1; y++) for (int cx = x0 / 16; cx <= x1 / 16; cx++)
                {
                    bool blend = HasFrontBlend(cx,y);
                    if (!blend)
                    {
                        var row = FrontRow(cx, y);
                        if (row.Children.Count != 0) scene.Children.Add(row);
                    }
                    else for (int x = cx * 16; x < Math.Min(Document.Width, cx * 16 + 16); x++)
                    {
                        var cell = Document.Get(x, y);
                        if (cell.Foreground is { Library: >= 0 } front)
                        {
                            int image = MapPreview.FrontImage(cell, front, DoorsOpen);
                            if (image < 0) continue;
                            int frames = front.Frames & 0x7F; if (frames > 1) animatedRows.Add((cx, y));
                            AppendSprite(scene, background, x, y, front.Library, image + (frames > 0 ? animationFrame / (1 + front.Tick) % frames : 0), true, true, MapSpriteDrawing.FrontBlend(front.Frames));
                        }
                    }
                }
            }
            scene.RetainRaster = true;
            detailScene=scene;detailSceneBuilds++;dc.DrawDrawing(scene);
        }
        dc.Pop(); dc.Pop();
    }
    private void DrawHoverCell(SceneContext dc)
    {
        if (!IsPointerOver) return;
        // Re-evaluate after camera changes, even when the mouse has not moved.
        var cell = CellAt(mouse);
        if (!Document.Contains(cell.X, cell.Y)) return;
        // Screen coordinates keep the stroke four pixels wide at every zoom.
        dc.DrawRectangle(null, hoverCellOutline, new(origin.X + cell.X * 48 * Zoom,
            origin.Y + cell.Y * 32 * Zoom, 48 * Zoom, 32 * Zoom));
    }
    private void DrawInteraction(SceneContext dc)
    {
        dc.PushTransform(new SceneTranslation(origin.X, origin.Y)); dc.PushTransform(new SceneScale(Zoom, Zoom));
        if(IsStampPickingTool&&!PastingRegion)DrawPickupHints(dc);
        DrawRegionPreview(dc);
        DrawSelectionMove(dc);
        if (Selection is Rect selection) dc.DrawRectangle(new ImmutableSolidColorBrush(Color.FromArgb(30, 110, 150, 255)), cursor, new(selection.X * 48, selection.Y * 32, selection.Width * 48, selection.Height * 32));
        bool stampPointer=(Tool is EditorTool.Stamp or EditorTool.TownStamp||IsStampPickingTool&&PickedStampId!=null)&&(IsPointerOver||hover!=(-1,-1));
        if ((Document.Contains(hover.X, hover.Y)||stampPointer) && !panning && !PastingRegion)
        {
            if (Tool == EditorTool.Wall) DrawWallBrush(dc);
            else if (Tool == EditorTool.SpecialBrush) DrawTreeBrush(dc);
            else if (Tool == EditorTool.TownStamp && Town != null) DrawStampPreview(dc,Town.Cells,hover.X & ~1,hover.Y & ~1);
            else if (Tool == EditorTool.Stamp && Stamp != null) DrawStampPreview(dc,Stamp,hover.X,hover.Y);
            else if(IsStampPickingTool)
            {
                if(PickedStamp is PlacedStamp picked){DrawStampOutline(dc,picked.Cells,picked.X,picked.Y,cursor);var target=PickedTarget(hover.X,hover.Y);DrawStampPreview(dc,picked.Cells,target.X,target.Y,Tool==EditorTool.Pickup?picked.Id:null);}
                else if(Document.StampAt(hover.X,hover.Y) is PlacedStamp over)DrawStampOutline(dc,over.Cells,over.X,over.Y,cursor);
            }
            else if ((Tool == EditorTool.Brush || Tool == EditorTool.Pencil) && SpriteIndex >= 0 && !IsCollisionLayer)
            {
                dc.PushOpacity(0.55);
                var layout = BrushLayout;
                if (Tool == EditorTool.Brush && layout != TilePatternLayout.None && Assets?.Libraries.TryGetValue(Library, out var atlas) == true)
                {
                    var key = (hover.X, hover.Y, BrushSize, Library, SpriteIndex, Layer, editRevision);
                    if (brushPreviewKey != key) { brushPreview = PlanBrush(hover.X, hover.Y, atlas.Count); brushPreviewKey = key; }
                    foreach (var tile in brushPreview)
                        DrawSprite(dc, tile.X, tile.Y, Library, Layer == MapLayer.Ground ? (tile.Cell.BackImage & 0x1FFFFFFF) - 1 : tile.Cell.MiddleImage - 1, Layer != MapLayer.Ground);
                }
                else
                {
                    var area = PaintFootprint(hover.X, hover.Y); int step = Layer == MapLayer.Ground ? 2 : 1;
                    for (int y = (int)area.Y; y < area.Bottom; y += step) for (int x = (int)area.X; x < area.Right; x += step)
                        if (Document.Contains(x, y)) DrawSprite(dc, x, y, Library, SpriteIndex, Layer != MapLayer.Ground);
                }
                dc.Pop();
            }
            if(Tool!=EditorTool.Wall&&Tool!=EditorTool.SpecialBrush&&Tool!=EditorTool.Stamp&&!IsStampPickingTool&&Tool!=EditorTool.Select&&Tool!=EditorTool.Eyedropper&&Tool!=EditorTool.Pan){var footprint = PaintFootprint(hover.X, hover.Y);
            dc.DrawRectangle(null, cursor, new(footprint.X * 48, footprint.Y * 32, footprint.Width * 48, footprint.Height * 32));}
        }
        dc.Pop(); dc.Pop();
        DrawHoverCell(dc);
    }
    private SceneGroup FrontRow(int cx, int y)
    {
        var key = (cx, y);
        if (frontRows.TryGetValue(key, out var existing)) return existing;
        var group = new SceneGroup();
        using (var dc = group.Open())
            for (int x = cx * 16; x < Math.Min(Document.Width, cx * 16 + 16); x++)
            {
                var cell = Document.Get(x, y);
                if (cell.Foreground is not { Library: >= 0 } front) continue;
                int image = MapPreview.FrontImage(cell, front, DoorsOpen);
                if (image < 0) continue;
                int frames = front.Frames & 0x7F;
                if (frames > 1) animatedRows.Add(key);
                DrawSprite(dc, x, y, front.Library, image + (frames > 1 ? animationFrame / (1 + front.Tick) % frames : 0), true, true);
            }
         frontRows[key] = group; return group;
    }
    private SceneGroup Chunk(int cx, int cy, int layer)
    {
        var key = (cx, cy, layer); if (chunks.TryGetValue(key, out var existing)) return existing; var group = new SceneGroup(); using (var dc = group.Open())
        {
            for (int y = cy * 16; y < Math.Min(Document.Height, cy * 16 + 16); y++) for (int x = cx * 16; x < Math.Min(Document.Width, cx * 16 + 16); x++)
            {
                var c = Document.Get(x, y); if (layer == 0 && x % 2 == 0 && y % 2 == 0) DrawSprite(dc, x, y, c.BackLibrary, (c.BackImage & 0x1FFFFFFF) - 1, false);
                if (layer == 1 && c.MiddleImage > 0 && c.MiddleLibrary >= 0) { int frames = MapSpriteDrawing.MiddleFrames(c.MiddleFrames); if (frames > 1) animatedChunks.Add(key); DrawSprite(dc, x, y, c.MiddleLibrary, c.MiddleImage - 1 + (frames > 1 ? animationFrame / (1 + c.MiddleTick) % frames : 0), true); }
            }
        }
         chunks[key] = group; return group;
    }
    private void AppendSprite(SceneGroup scene, MapSpriteDrawing.BackgroundIndex background, int x, int y, int lib, int index, bool anchored, bool front, bool blend)
    {
        if (index < 0 || MapSpriteDrawing.SkipFront(lib, Document.SourceFormat) || Assets?.Get(lib, index) is not Sprite sprite) return;
        var bounds=MapSpriteDrawing.Bounds(sprite,x,y,lib,index,anchored,front,blend,Document.SourceFormat);
        if(blend&&!bounds.Intersects(detailCoverage))return;
        MapSpriteDrawing.Append(scene,sprite,bounds,blend,detailEffects,background);drawn++;
    }
    private void DrawSprite(SceneContext dc, int x, int y, int lib, int index, bool anchored) => DrawSprite(dc, x, y, lib, index, anchored, false);
    private void DrawSprite(SceneContext dc, int x, int y, int lib, int index, bool anchored, bool front, bool blend = false)
    {
        if (index < 0 || MapSpriteDrawing.SkipFront(lib, Document.SourceFormat)) return; var s = Assets?.Get(lib, index); if (s == null) return;
        var bounds = MapSpriteDrawing.Bounds(s, x, y, lib, index, anchored, front, blend, Document.SourceFormat);
        dc.DrawImage(blend ? MapSpriteDrawing.Additive(s, bounds, new SceneGroup()) : s.Bitmap, bounds); drawn++;
    }
    public (int X, int Y) CellAt(Point p) => ((int)Math.Floor((p.X - origin.X) / Zoom / 48), (int)Math.Floor((p.Y - origin.Y) / Zoom / 32));
    private int BrushSeed(int x, int y) => HashCode.Combine(Library, SpriteIndex, Layer, BrushSize, Layer == MapLayer.Ground ? x & ~1 : x, Layer == MapLayer.Ground ? y & ~1 : y);
    public List<StampCell> PlanBrush(int x, int y, int atlasCount)
    {
        // Read existing tiles directly and write only the planned changes to a sparse overlay.
        // This also handles long diagonal joins without truncating the preview neighbourhood.
        var copy = new MapDocument(Document.Width, Document.Height);
        var changed = new HashSet<(int X, int Y)>();
        new AutoTileBrush(copy, new(), Layer, Library, SpriteIndex, atlasCount, BrushLayout, (cx, cy) => changed.Add((cx, cy)), BrushSeed(x, y), Document.Get).Paint(x, y, BrushSize);
        return changed.OrderBy(p => p.Y).ThenBy(p => p.X).Select(p => new StampCell(p.X, p.Y, copy.Get(p.X, p.Y))).ToList();
    }
    private bool BeginSelection(int x, int y)
    {
        if (SelectionContains(x, y)) return BeginSelectionMove(x, y);
        if (!SelectionContains(x, y)) Deselect();
        if (!Document.Contains(x, y)) return false;
        selectionStart = (x, y); selectionDragged = false;
        return true;
    }
    public bool PickTile(int x, int y)
    {
        if (!Document.Contains(x, y) || Layer is not (MapLayer.Ground or MapLayer.Middle)) return false;
        if (Layer == MapLayer.Ground) { x &= ~1; y &= ~1; }
        var cell = Document.Get(x, y);
        int library = Layer == MapLayer.Ground ? cell.BackLibrary : cell.MiddleLibrary;
        int image = Layer == MapLayer.Ground ? (cell.BackImage & 0x1FFFFFFF) - 1 : cell.MiddleImage - 1;
        if (library < 0 || image < 0) return false;
        Library = library;
        SpriteIndex = image;
        AssetPicked?.Invoke(library, image, Layer);
        return true;
    }
    private void UpdateSelection(int x, int y)
    {
        if (movingSelection != null) { UpdateSelectionMove(x, y); return; }
        if ((x, y) != selectionStart) selectionDragged = true;
        if (!selectionDragged) return;
        int ax = selectionStart.X, ay = selectionStart.Y;
        int bx = Math.Clamp(x, 0, Document.Width - 1), by = Math.Clamp(y, 0, Document.Height - 1);
        Selection = new(Math.Min(ax, bx), Math.Min(ay, by), Math.Abs(bx - ax) + 1, Math.Abs(by - ay) + 1);
    }
    protected override void OnPointerPressed(PointerPressedEventArgs e)
    {
        base.OnPointerPressed(e); Focus(); mouse = e.GetPosition(this);
        hover = CellAt(mouse); if(e.GetCurrentPoint(this).Properties.IsRightButtonPressed) { if (PickTile(hover.X, hover.Y)) { e.Handled = true; return; } if (!SelectionContains(hover.X, hover.Y)) Deselect(); return; } panning = e.GetCurrentPoint(this).Properties.IsMiddleButtonPressed || InputState.IsKeyDown(Key.Space) || Tool == EditorTool.Pan && !PastingRegion && e.GetCurrentPoint(this).Properties.IsLeftButtonPressed;
        if (panning) { panStart = mouse; originStart = origin; CapturePointer(e.Pointer); dragging = true; return; }
        if (!e.GetCurrentPoint(this).Properties.IsLeftButtonPressed) return;
        if (Tool == EditorTool.Eyedropper && !PastingRegion) { PickTile(hover.X, hover.Y); e.Handled = true; return; }
        if(PastingRegion){PlaceRegion();e.Handled=true;return;}
        if(IsStampPickingTool){if(PickedStampId==null)PickupAt(hover.X,hover.Y);else if(Tool==EditorTool.Clone)ClonePickedTo(hover.X,hover.Y);else MovePickedTo(hover.X,hover.Y);e.Handled=true;return;}
        if (Tool == EditorTool.TownStamp) { PlaceTownAt(hover.X, hover.Y); e.Handled = true; return; }
        if (Tool == EditorTool.Bucket) { BucketFill(hover.X, hover.Y); e.Handled = true; return; }
        if (Tool == EditorTool.Select)
        {
            if (!BeginSelection(hover.X, hover.Y)) return;
        }
        else Deselect();
        dragging = true; CapturePointer(e.Pointer); History.Begin(); lastPaint = (-999, -999); BeginWallStroke(hover.X, hover.Y);
        if (Tool != EditorTool.Select) Apply(hover.X, hover.Y);
        InvalidateVisual();
    }
    protected override void OnPointerMoved(PointerEventArgs e)
    {
        base.OnPointerMoved(e); mouse = e.GetPosition(this);
        var next=CellAt(mouse);
        if(next==hover&&!dragging&&!sceneDirty)return;
        hover = next; Hover?.Invoke(hover.X, hover.Y);
        if (dragging && panning) origin = originStart + (mouse - panStart);
        else if (dragging&&Tool!=EditorTool.Stamp&&!IsStampPickingTool)
        {
            if (Tool == EditorTool.Select) UpdateSelection(hover.X, hover.Y);
            else if (Tool == EditorTool.Wall) PaintWallStroke(hover.X, hover.Y);
            else
            { // Interpolate strokes so fast pointer movement leaves no gaps.
                if (lastPaint.X > -999 && Tool != EditorTool.Stamp) { int dx = hover.X - lastPaint.X, dy = hover.Y - lastPaint.Y, n = Math.Max(Math.Abs(dx), Math.Abs(dy)); var start = lastPaint; for (int i = 1; i <= Math.Min(n, 4096); i++) Apply(start.X + (int)Math.Round(dx * i / (double)n), start.Y + (int)Math.Round(dy * i / (double)n)); } else Apply(hover.X, hover.Y);
            }
        }
        RefreshPointerScene();
    }
    protected override void OnPointerExited(PointerEventArgs e)
    {
        base.OnPointerExited(e);
        if(!dragging){hover=(-1,-1);RefreshPointerScene();}
    }
    protected override void OnPointerReleased(PointerReleasedEventArgs e) { base.OnPointerReleased(e); if(e.InitialPressMouseButton != MouseButton.Right) { if (movingSelection != null && e.InitialPressMouseButton == MouseButton.Left) { var cell = CellAt(e.GetPosition(this)); UpdateSelectionMove(cell.X, cell.Y); FinishSelectionMove(); } End(); } }
    protected override void OnPointerCaptureLost(PointerCaptureLostEventArgs e) { base.OnPointerCaptureLost(e); NavigationCancelled?.Invoke(); CancelSelectionMove(); lastWall = null; wallStrokePath.Clear(); if (dragging) { dragging = false; panning = false; History.Commit(); Changed?.Invoke(); } }
    private void End() { CancelSelectionMove(); History.Commit(); dragging = false; panning = false; lastWall = null; wallStrokePath.Clear(); ReleasePointer(); Changed?.Invoke(); InvalidateVisual(); }
    protected override void OnPointerWheelChanged(PointerWheelEventArgs e) { base.OnPointerWheelChanged(e); var p = e.GetPosition(this); var world = (p - origin) / Zoom; Zoom = LimitZoom(Zoom * (e.Delta.Y > 0 ? 1.15 : 1 / 1.15)); origin = p - world * Zoom; InvalidateVisual(); Changed?.Invoke(); e.Handled = true; }
    private void Apply(int x, int y)
    {
        if (!AllowEdit(Tool is EditorTool.Stamp or EditorTool.SpecialBrush or EditorTool.Wall ? null : Layer)) return;
        if (Tool == EditorTool.Wall) { PaintWallStroke(x, y); return; }
        if ((x, y) == lastPaint) return;
        if(Tool==EditorTool.Stamp&&Stamp!=null)
        {
            lastPaint=(x,y);
            var placement=GetStampPreviewPlacement(Stamp,x,y);
            if(!placement.Allowed){Message?.Invoke(LocalText.Placement(placement));InvalidateVisual();return;}
            var placed=Document.PlaceStamp(Stamp,x,y,StampName,History,InvalidateCell);
            if (placed != null) AssetUsed?.Invoke();
            Message?.Invoke(((placed==null ? (LocalText.T("Placement blocked \u00b7 Stamp data or instance limit prevents placement")) : (LocalText.Format("{0} placed", StampName)))));
            return;
        }
        if (!Document.Contains(x, y)) return; lastPaint = (x, y);
        if (Tool == EditorTool.SpecialBrush) { PaintTrees(x, y); return; }
        if (Tool != EditorTool.Brush && Tool != EditorTool.Pencil && Tool != EditorTool.Eraser) return;
        if (Tool != EditorTool.Eraser && !IsCollisionLayer && SpriteIndex < 0) return;
        var layout = BrushLayout;
        if (Tool == EditorTool.Brush && layout != TilePatternLayout.None && Assets?.Libraries.TryGetValue(Library, out var atlas) == true)
        {
            new AutoTileBrush(Document, History, Layer, Library, SpriteIndex, atlas.Count, layout, InvalidateCell, BrushSeed(x, y)).Paint(x, y, BrushSize);
            AssetUsed?.Invoke();
            return;
        }
        int size = Tool is EditorTool.Pencil or EditorTool.Bucket ? 1 : BrushSize, radius = size / 2;
        int step = Layer == MapLayer.Ground ? 2 : 1;
        if (step == 2) { x &= ~1; y &= ~1; }
        for (int iy = -radius; iy <= radius; iy++) for (int ix = -radius; ix <= radius; ix++)
        {
            int sx = x + ix * step, sy = y + iy * step;
            if (!Document.Contains(sx, sy)) continue; History.Record(Document, sx, sy); var c = Document.Get(sx, sy).Copy(); bool erase = Tool == EditorTool.Eraser; int image = erase ? 0 : SpriteIndex + 1;
            switch (Layer) { case MapLayer.Ground: c.BackLibrary = (short)Library; c.BackImage = (c.BackImage & 0x60000000) | image; break; case MapLayer.Middle: c.MiddleLibrary = (short)Library; c.MiddleImage = checked((ushort)image); c.MiddleFrames = 0; c.MiddleTick = 0; break; case MapLayer.Objects: c.Foreground = erase ? null : new((short)Library, checked((short)image)); break; case MapLayer.LowCollision: case MapLayer.HighCollision: c.BackImage = erase ? c.BackImage & ~ActiveCollisionMask : c.BackImage | ActiveCollisionMask; break; }
            Document.Set(sx, sy, c); InvalidateCell(sx, sy);
        }
        if (Tool != EditorTool.Eraser && !IsCollisionLayer) AssetUsed?.Invoke();
    }
    public int BucketFill(int x, int y)
    {
        if (!AllowEdit(Layer)) return 0;
        bool collision = IsCollisionLayer;
        if (!collision && Layer is not (MapLayer.Ground or MapLayer.Middle))
        {
            Message?.Invoke(LocalText.T("Fill works on ground tiles, middle tiles (smtiles), and low/high collisions"));
            return 0;
        }
        if (!Document.Contains(x, y) || !collision && SpriteIndex < 0) return 0;
        int image = collision ? ActiveCollisionMask : checked(SpriteIndex + 1);
        if (Layer == MapLayer.Middle && image > ushort.MaxValue) return 0;
        int step = Layer == MapLayer.Ground ? 2 : 1;
        if (step == 2) { x &= ~1; y &= ~1; }
        (int Library, int Image) FillValue(Cell c) => collision ? (0, c.BackImage & ActiveCollisionMask) : Layer == MapLayer.Ground
            ? (c.BackLibrary, c.BackImage & 0x1FFFFFFF) : (c.MiddleLibrary, c.MiddleImage);
        var source = FillValue(Document.Get(x, y));
        if (source == (collision ? 0 : (int)(short)Library, image)) return 0;
        var pending = new Queue<(int X, int Y)>();
        var visited = new HashSet<int>();
        void Enqueue(int cx, int cy)
        {
            if (Document.Contains(cx, cy) && visited.Add(cy * Document.Width + cx)
                && FillValue(Document.Get(cx, cy)) == source) pending.Enqueue((cx, cy));
        }
        Enqueue(x, y);
        History.Begin();
        int count = 0;
        while (pending.TryDequeue(out var point))
        {
            var c = Document.Get(point.X, point.Y).Copy();
            History.Record(Document, point.X, point.Y);
            if (collision) c.BackImage |= ActiveCollisionMask;
            else if (Layer == MapLayer.Ground)
            {
                c.BackLibrary = (short)Library;
                c.BackImage = (c.BackImage & 0x60000000) | image;
            }
            else
            {
                c.MiddleLibrary = (short)Library;
                c.MiddleImage = (ushort)image;
                c.MiddleFrames = c.MiddleTick = 0;
            }
            Document.Set(point.X, point.Y, c);
            InvalidateCell(point.X, point.Y);
            count++;
            Enqueue(point.X - step, point.Y); Enqueue(point.X + step, point.Y);
            Enqueue(point.X, point.Y - step); Enqueue(point.X, point.Y + step);
        }
        History.Commit();
        if (!collision) AssetUsed?.Invoke();
        Changed?.Invoke();
        Message?.Invoke(((collision ? (LocalText.Format("Applied {0} collision to {1:N0} cells", LocalText.T(Layer == MapLayer.LowCollision ? "low" : "high"), count)) : (LocalText.Format("Filled {0:N0} tiles", count)))));
        return count;
    }
    public void FillSelection() { if (!AllowEdit(Layer)) return; if (Selection is not Rect r || SpriteIndex < 0 && !IsCollisionLayer) return; History.Begin(); var previous = Tool; Tool = EditorTool.Pencil; lastPaint = (-999, -999); for (int y = (int)r.Y; y < r.Bottom; y++) for (int x = (int)r.X; x < r.Right; x++) Apply(x, y); Tool = previous; History.Commit(); Changed?.Invoke(); }
    internal byte? CommonMetadata(Rect area, Func<Cell, byte> read)
    {
        byte? value = null;
        for (int y = Math.Max(0, (int)area.Y); y < Math.Min(Document.Height, area.Bottom); y++)
        for (int x = Math.Max(0, (int)area.X); x < Math.Min(Document.Width, area.Right); x++)
        {
            byte next = read(Document.Get(x, y));
            if (value.HasValue && value != next) return null;
            value = next;
        }
        return value;
    }
    public void ApplyMetadata(byte? light, byte? door, byte? offset, Rect? areaOverride = null)
    {
        if (!AllowEdit(MapLayer.Objects)) return;
        if (light == null && door == null && offset == null) return;
        Rect area = areaOverride ?? Selection ?? new Rect(hover.X, hover.Y, 1, 1);
        History.Begin();
        for (int y = Math.Max(0, (int)area.Y); y < Math.Min(Document.Height, area.Bottom); y++)
        for (int x = Math.Max(0, (int)area.X); x < Math.Min(Document.Width, area.Right); x++)
        {
            var original = Document.Get(x, y);
            var cell = original.Copy();
            if (light.HasValue) cell.Light = light.Value;
            if (door.HasValue) cell.Door = door.Value;
            if (offset.HasValue) cell.DoorOffset = offset.Value;
            if (cell.SameContents(original)) continue;
            History.Record(Document, x, y); Document.Set(x, y, cell); InvalidateCell(x, y);
        }
        History.Commit(); RefreshAssets(); Changed?.Invoke();
    }
    public void ClearSelection() { if (!AllowEdit(Layer)) return; brushPreviewKey = null; if (Selection is not Rect r) return; History.Begin(); for (int y = (int)r.Y; y < r.Bottom; y++) for (int x = (int)r.X; x < r.Right; x++) { History.Record(Document, x, y); var c = Document.Get(x, y).Copy(); switch (Layer) { case MapLayer.Ground: c.BackImage &= 0x60000000; break; case MapLayer.Middle: c.MiddleImage = 0; c.MiddleFrames = 0; c.MiddleTick = 0; break; case MapLayer.Objects: c.Foreground = null; break; case MapLayer.LowCollision: case MapLayer.HighCollision: c.BackImage &= ~ActiveCollisionMask; break; } Document.Set(x, y, c); InvalidateCell(x,y); } History.Commit(); chunks.Clear(); frontRows.Clear(); InvalidateVisual(); Changed?.Invoke(); }
}





