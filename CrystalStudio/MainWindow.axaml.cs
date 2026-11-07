using System.Collections.ObjectModel;

using System.ComponentModel;

using System.IO;

namespace CrystalStudio;

public record LibraryChoice(int Id, string Label) { public override string ToString() => Label; }

public sealed class AssetChoice : INotifyPropertyChanged

{

    public int Index { get; init; }

    public string Label { get; init; } = "";
    public string Detail { get; init; } = "";
    public string DisplayLabel => StampPath != null || Asset?.StampPath != null || Asset is not { Pattern: not TilePatternLayout.None } tile
        ? Label : LocalText.Format("Set {0}{1}", tile.Index / AutoTileBrush.BlockSize(tile.Pattern), tile.Pattern == TilePatternLayout.Mir3 && tile.Index % 30 == 5 ? " B" : "");
    public string DisplayDetail => Asset is { StampPath: null } tile && Detail.Length > 0
        ? LocalText.Format("{0} · Lib {1}", LocalText.T(tile.Layer.ToString()), tile.Library) : LocalText.T(Detail);
    public string DisplayDescription => Asset is { StampPath: null } tile
        ? LocalText.Format("{0} {1} · {2} · Library {3}\n{4}", LocalText.T(tile.Pattern == TilePatternLayout.None ? "Sprite" : "Terrain set"), DisplayLabel, LocalText.T(tile.Layer.ToString()), tile.Library, tile.Source)
            + (Description.EndsWith("Unavailable in the current library folder", StringComparison.Ordinal) ? "\n" + LocalText.T("Unavailable in the current library folder") : "")
        : Description;
    public bool DetailVisible => Detail.Length == 0 ? false : true;

    private string description = "";

    public string Description { get => description; set { description = value; PropertyChanged?.Invoke(this, new(nameof(Description))); PropertyChanged?.Invoke(this, new(nameof(DisplayDescription))); } }

    public string? StampPath { get; init; }

    public List<StampCell>? StampCells { get; set; }
    public string? LoadError { get; set; }

    public TilePatternLayout PreviewLayout { get; init; }

    public AssetReference? Asset { get; init; }
    private bool isFavourite;
    public bool IsFavourite { get => isFavourite; set { isFavourite = value; PropertyChanged?.Invoke(this, new(nameof(PinLabel))); PropertyChanged?.Invoke(this, new(nameof(PinHint))); } }
    public string PinLabel => IsFavourite ? "\u2605" : "\u2606";
    public string PinHint => LocalText.T(IsFavourite ? "Unpin from Favourites" : "Pin to Favourites");
    public bool CanPin => Asset?.Valid == true;

    private RasterImage? bitmap;

    public RasterImage? Bitmap { get => bitmap; set { bitmap = value; PropertyChanged?.Invoke(this, new(nameof(Bitmap))); } }

    public event PropertyChangedEventHandler? PropertyChanged;

}

public partial class MainWindow : EditorWindow

{

    private AssetStore? assets;

    private string? mapPath { get => activeMap?.Path; set { if (activeMap != null) activeMap.Path = value; } }

    private int page;

    private string? stampFolder;

    private List<StampEntry> stampEntries = [];

    private bool stampIndexLoading;

    private string? selectedStampPath;

    private string? selectedStampName;

    private RasterImage? selectedStampPreview;

    private int catalogVersion, pageVersion;

    private readonly SemaphoreSlim stampReaders = new(2);

    private bool StampMode => Viewport.Tool == EditorTool.Stamp;

    private int PageSize => CategoryPalette ? CategoryPageSize : SavedPalette || StampMode ? 24 : 96;

    private int filteredCount;

    private readonly DispatcherTimer searchDelay = new() { Interval = TimeSpan.FromMilliseconds(150) };

    private bool busy;
    private bool closed;
    private EditorWindow? shortcutsWindow;

    private bool initialized;

    private readonly ObservableCollection<AssetChoice> choices = [];

    private readonly DispatcherTimer refresh = new() { Interval = TimeSpan.FromMilliseconds(40) };

    private int pendingRefresh;

    private readonly SettingsStore settingsStore;

    private LibrarySettings settings = new();

    private bool startupComplete;

    private readonly AssetHistoryStore assetHistory;
    private TabControl? paletteTabs;
    private bool restoringAsset;
    private bool SavedPalette => paletteTabs?.SelectedIndex is 1 or 2;
    private bool FavouritesPalette => paletteTabs?.SelectedIndex == 1;

    public MainWindow() : this(new AssetHistoryStore()) { }

    public MainWindow(AssetHistoryStore historyStore, RecentMapStore? recentMapStore = null, SettingsStore? settingsProvider = null)

    {

        settingsStore = settingsProvider ?? new SettingsStore();
        assetHistory = historyStore;
        recentMaps = recentMapStore ?? new RecentMapStore();
        InitializeComponent(); ThemeService.Initialize(); DarkModeToggle.IsChecked = ThemeService.ForceDark; initialized = true; AssetList.ItemsSource = choices;
        InitializeLanguageMenu();

        try { assetHistory.Load(); } catch (Exception ex) { StudioLog.Error("Assets.History",ex); Status.Text = LocalText.Format("Could not read saved assets: {0}",ex.Message); }

        searchDelay.Tick += (_, _) => { searchDelay.Stop(); page = 0; BuildPage(); };

        Viewport.Hover += UpdateCellInspector;
        Viewport.Changed += () => UpdateCellInspector(Viewport.HoverCell.X, Viewport.HoverCell.Y);
        InitializeCellHover();
        Viewport.AssetPicked += SelectPickedTile;

        Viewport.Changed += UpdateLabels; Viewport.Message += message => Status.Text = (message); Viewport.Rendered += (ms, count) => { Performance.Text = LocalText.Format("Prepare {0:F1} ms · Batches {1:N0} · Sprite cache {2:N0}",ms,count,(assets?.Cached ?? 0) + (assets?.OverviewCached ?? 0)); ToolTip.SetTip(LibraryStatus, (Performance.Text)); };
        Viewport.AssetUsed += RememberActiveAsset;

        refresh.Tick += (_, _) => { refresh.Stop(); if (Interlocked.Exchange(ref pendingRefresh, 0) == 0 || closed) return; Viewport.RefreshAssets(); RefreshThumbnails(); };

        KeyDown += KeyDownHandler; Closing += OnClosing; Closed += (_, _) => { closed = true; catalogVersion++; pageVersion++; refresh.Stop(); searchDelay.Stop(); Viewport.StopBackgroundWork(); assets?.Dispose(); }; Loaded += async (_, _) =>

        {

            if (startupComplete) return;

            startupComplete = true;

            await Run(async () =>

            {

                settings = settingsStore.Load();
                ApplySetNumbering(settings.ZeroBasedSetNumbers);

                if (string.IsNullOrWhiteSpace(settings.LibraryFolder)) OpenSettings(true);

                else

                {

                    try { await LoadLibraries(settings.LibraryFolder); }

                    catch (Exception ex) { StudioLog.Error("Startup.Libraries",ex,$"Root={settings.LibraryFolder}; Format={settings.AssetFormat}"); Status.Text = LocalText.T("Saved Library Folder Unavailable \u2014 Open Settings to Change It"); ToolTip.SetTip(Status, (ex.Message)); }

                }

                await RefreshMapBrowserAsync(settings.LibraryFolder);

                if (assets == null) await DiscoverStampsAsync();

            });

            if (Environment.GetCommandLineArgs().Skip(1).FirstOrDefault() is string path && File.Exists(path)) await Run(() => OpenMap(path));
            StartRecovery();

        };

        InitializeDocking();

        InitializeMapTabs();
        InitializeMapCreation();
        InitializeRecentMaps();

        InitializeCellMenu();
        InitializeLayerLocks();

        ThemeService.Changed += ThemeChanged;

        Closed += (_, _) => ThemeService.Changed -= ThemeChanged;

        UpdateLabels();

    }

    private void ThemeChanged() => Viewport.InvalidateVisual();

    private readonly List<Grid> inspectorRows = [];
    private (MapDocument Document, long Revision, AssetStore? Assets, int X, int Y)? inspectorCell;
    private void UpdateCellInspector(int x, int y)
    {
        var next = (Viewport.Document, Viewport.Document.PickupRevision, assets, x, y);
        if (inspectorCell == next) return;
        inspectorCell = next;
        if (!Viewport.Document.Contains(x, y))
        {
            Inspector.Text = LocalText.T("Outside map");
            foreach (var row in inspectorRows) row.IsVisible = false;
            InspectorDetails.Tag = null;
            Coordinates.Text = ("\u2014, \u2014");
            return;
        }
        Coordinates.Text = LocalText.Format("Cell {0:N0}, {1:N0}",x,y);
        var c = Viewport.Document.Get(x, y);
        string ImageInfo(int library, int image)
        {
            if (image == 0) return LocalText.T("None");
            return LocalText.Format("Lib {0} \u00b7 Image {1} / Sprite {2}", library, image, image - 1);
        }
        string LibraryInfo(int library) => assets?.Libraries.TryGetValue(library, out var atlas) == true
            ? LocalText.Format("Library {0}: {1}", library, atlas.Path) : LocalText.Format("Library {0}: unavailable", library);
        var groups = new List<(string Title, string Detail, string Hint)>
        {
            ("Collision", LocalText.Format("Low {0} \u00b7 High {1}", LocalText.T((c.BackImage & 0x20000000) != 0 ? "blocked" : "clear"), LocalText.T((c.BackImage & 0x40000000) != 0 ? "blocked" : "clear")), "Low and High Collision Flags"),
            ("Ground", ImageInfo(c.BackLibrary, c.BackImage & 0x1FFFFFFF), LibraryInfo(c.BackLibrary)),
            ("Middle", LocalText.Format("{0}\nFrames {1} \u00b7 Tick {2}", ImageInfo(c.MiddleLibrary, c.MiddleImage), c.MiddleFrames, c.MiddleTick), LibraryInfo(c.MiddleLibrary))
        };
        if (c.ForegroundImages.Count == 0) groups.Add(("Objects", LocalText.T("None"), "No Foreground Images"));
        for (int i = 0; i < c.ForegroundImages.Count; i++)
        {
            var f = c.ForegroundImages[i];
            // Front.Frames packs the additive blend flag into bit 0x80; show the plain
            // animation length so an animated light reads "10+blend", not "138 frames".
            string framesText = (f.Frames & 0x80) != 0
                ? (f.Frames & 0x7F) + "+blend" : (f.Frames & 0x7F).ToString();
            groups.Add((LocalText.Format("Object {0}", i + 1), LocalText.Format("{0}\nFrames {1} \u00b7 Tick {2} \u00b7 Flag {3}", ImageInfo(f.Library, f.Image & 0x7FFF), framesText, f.Tick, LocalText.T((f.Image & 0x8000) != 0 ? "on" : "off")), LibraryInfo(f.Library)));
        }
        groups.Add(("Metadata", LocalText.Format("Light {0} \u00b7 Door {1} \u00b7 Offset {2}", c.Light, c.Door, c.DoorOffset), "Light Value, Door Index and Door Offset"));
        groups.Add(("Tile Animation", LocalText.Format("Image {0} \u00b7 Offset {1} \u00b7 Frames {2}", c.TileAnimation, c.TileOffset, c.TileFrames), "Tile Animationation image, offset and frame count"));
        UpdateCellHoverContent(x, y, c, groups);
        Inspector.Text = LocalText.Format("Cell {0:N0}, {1:N0} · {2} {3}",x,y,c.ForegroundImages.Count,LocalText.T(c.ForegroundImages.Count == 1 ? "object" : "objects"));
        string key = string.Join("\n", groups);
        if (Equals(InspectorDetails.Tag, key)) return;
        InspectorDetails.Tag = key;
        for (int i = 0; i < groups.Count; i++)
        {
            if (i == inspectorRows.Count)
            {
                var row = new Grid { Margin = new Thickness(0, 0, 8, 2) };
                row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(64) });
                row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
                row.Children.Add(new TextBlock { FontSize = 11, FontWeight = FontWeight.SemiBold });
                var value = new TextBlock { TextWrapping = TextWrapping.Wrap, FontSize = 11, LineHeight = 14 };
                value.SetResource(TextBlock.ForegroundProperty, "Muted");
                Grid.SetColumn(value, 1); row.Children.Add(value);
                inspectorRows.Add(row); InspectorDetails.Children.Add(row);
            }
            var group = inspectorRows[i];
            var (title, detail, hint) = groups[i];
            group.IsVisible = true; ToolTip.SetTip(group, LocalText.T(hint));
            (((TextBlock)group.Children[0])).Text = LocalText.T(title);
            (((TextBlock)group.Children[1])).Text = (detail);
        }
        for (int i = groups.Count; i < inspectorRows.Count; i++) inspectorRows[i].IsVisible = false;
    }

    private void ConfigureResponsivePalette(Grid palette)
    {
        var header = (Control)LibraryList.Parent;
        var paging = (Control)PreviousButton.Parent;
        palette.ColumnDefinitions.Add(new ColumnDefinition());
        palette.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(0) });
        bool wide = false;
        palette.SizeChanged += (_, _) =>
        {
            bool next = palette.Bounds.Width >= 540 || (palette.Bounds.Width >= 280 && palette.Bounds.Height < 240) || (palette.Bounds.Width >= 400 && palette.Bounds.Width > palette.Bounds.Height * 1.7);
            if (wide == next) return;
            wide = next;
            palette.ColumnDefinitions[0].MaxWidth = wide ? 210 : double.PositiveInfinity;
            palette.ColumnDefinitions[0].Width = new GridLength(wide ? 0.45 : 1, GridUnitType.Star);
            palette.ColumnDefinitions[1].Width = wide ? new GridLength(0.55, GridUnitType.Star) : new GridLength(0);
            for (int i = 0; i < palette.RowDefinitions.Count; i++) palette.RowDefinitions[i].Height = new GridLength(0);
            palette.RowDefinitions[wide ? 0 : 2].Height = wide ? new GridLength(1, GridUnitType.Star) : GridLength.Auto;
            palette.RowDefinitions[wide ? 1 : 3].Height = wide ? GridLength.Auto : new GridLength(1, GridUnitType.Star);
            if (!wide) palette.RowDefinitions[4].Height = GridLength.Auto;
            Grid.SetRow(header, wide ? 0 : 2); Grid.SetRowSpan(header, wide ? 2 : 1);
            Grid.SetRow(AssetArea, wide ? 0 : 3); Grid.SetColumn(AssetArea, wide ? 1 : 0);
            Grid.SetRow(paging, wide ? 1 : 4); Grid.SetColumn(paging, wide ? 1 : 0);
            header.Margin = wide ? new Thickness(0, 0, 12, 0) : new Thickness(0);
        };
    }

    private void InitializeDocking()

    {

        var root = (Grid)Content;

        var toolBorder = (Border)ToolPanel.Parent;

        var oldWorkspace = (Grid)toolBorder.Parent;

        var canvas = (Grid)Viewport.Parent;

        var palette = (Grid)AssetArea.Parent;

        var layers = (Control)palette.Children[0];

        var selected = (StackPanel)palette.Children[palette.Children.Count - 1];

        var active = (Control)selected.Children[0];

        var inspectorExpander = (Expander)selected.Children[1];

        var inspector = (Control)inspectorExpander.Content;

        inspectorExpander.Content = null;

        selected.Children.Clear();

        palette.Children.Remove(layers);

        palette.Children.Remove(selected);

        // Collapse the vacated layer rows, keeping the palette header and library selector.

        palette.RowDefinitions[0].Height = new GridLength(0);

        palette.RowDefinitions[1].Height = new GridLength(0);

        palette.RowDefinitions[5].Height = new GridLength(0);

        oldWorkspace.Children.Remove(canvas);

        toolBorder.Child = null;

        ((Border)palette.Parent).Child = null;

        var options = (Border)root.Children.Cast<Control>().First(c => Grid.GetRow(c) == 2 && c is Border);

        root.Children.Remove(options);

        root.Children.Remove(oldWorkspace);

        root.RowDefinitions[2].Height = new GridLength(0);

        Grid.SetRow(BusyProgress, 0);

        var docking = new DockWorkspace(this, canvas, KeyDownHandler, defaultLayout: DockWorkspace.LoadDefaultLayout());

        Grid.SetRow(docking, 3); root.Children.Add(docking);

        ToolPanel.Margin = new Thickness(5);

        docking.EnableLeftTabs();
        docking.Add("Maps", CreateMapBrowser(), Dock.Left, 2);

        docking.Add("Tools", new ScrollViewer { Content = ToolPanel, VerticalScrollBarVisibility = ScrollBarVisibility.Auto }, Dock.Left, 1);

        docking.Add("Tool Options", new ScrollViewer { Content = options, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled, VerticalScrollBarVisibility = ScrollBarVisibility.Auto }, Dock.Left, 0.15);

        layers.Margin = new Thickness(10, 5, 10, 5);

        docking.Add("Layers", new ScrollViewer { Content = layers, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled }, Dock.Left, 1.9);

        var minimap = new MapMinimap { Viewport = Viewport, Margin = new Thickness(10) };
        RegisterEditorName("Minimap", minimap);
        docking.Add("Minimap", minimap, Dock.Right, 1.2);

        palette.Margin = new Thickness(10);
        ConfigureResponsivePalette(palette);

        docking.Add("Palette", CreatePaletteTabs(palette), Dock.Right, 3);
        docking.Add(LocalText.T("Composition Models"), CreateSetCompositionPanel(), Dock.Right, 3.5);

        active.Margin = new Thickness(10);

        var activeGrid = (Grid)((Border)active).Child;
        var activeCaption = (Control)activeGrid.Children[1];
        var previewFrame = (Border)SelectedPreview.Parent;
        previewFrame.Height = double.NaN;
        RenderOptions.SetBitmapInterpolationMode(SelectedPreview, Avalonia.Media.Imaging.BitmapInterpolationMode.None);
        activeGrid.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        activeGrid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        active.SizeChanged += (_, _) =>
        {
            bool horizontal = active.Bounds.Width >= 240 && active.Bounds.Height < 160;
            activeGrid.ColumnDefinitions[0].Width = new GridLength(horizontal ? 0.6 : 1, GridUnitType.Star);
            activeGrid.ColumnDefinitions[1].Width = horizontal ? new GridLength(0.4, GridUnitType.Star) : new GridLength(0);
            activeGrid.RowDefinitions[1].Height = horizontal ? new GridLength(0) : GridLength.Auto;
            Grid.SetRow(activeCaption, horizontal ? 0 : 1);
            Grid.SetColumn(activeCaption, horizontal ? 1 : 0);
            activeCaption.Margin = horizontal ? new Thickness(10, 0, 0, 0) : new Thickness(0, 8, 0, 0);
        };

        docking.Add("Active Asset", active, Dock.Right, 0.9);

        inspector.Margin = new Thickness(6);

        docking.Add("Cell Inspector", new ScrollViewer { Content = inspector, VerticalScrollBarVisibility = ScrollBarVisibility.Auto }, Dock.Left, 1);

        var view = ViewMenu;

        view.Items.Add(new Separator());

        docking.AddSectionMenu(view);

        view.Items.Add(new Separator());

        var reset = new MenuItem { Header = LocalText.T("Reset Panel Layout")};

        var lockLayout = new MenuItem { Header = LocalText.T("Lock Panel Layout"), ToggleType = MenuItemToggleType.CheckBox };
        lockLayout.Click += (_, _) => docking.SetLayoutLocked(lockLayout.IsChecked);
        docking.LayoutLockChanged += (_, _) => { lockLayout.IsChecked = docking.IsLayoutLocked; reset.IsEnabled = !docking.IsLayoutLocked; };
        view.Items.Add(lockLayout);

        reset.Click += (_, _) => docking.Reset(); view.Items.Add(reset);

        Loaded += (_, _) => docking.Restore();

    }

    private void UpdateLabels() { if (!initialized || Viewport == null) return; UpdateLayerCounts(); ZoomLabel.Content = ($"{Viewport.Zoom:P0}"); SelectedLabel.Text = (Viewport.Tool == EditorTool.TownStamp ? (Viewport.Town?.Name ?? LocalText.T("Generate a Town")) : (Viewport.Tool == EditorTool.Wall ? (Viewport.WallCatalogueError ?? LocalText.Format("Wall · {0} · Drag to connect · Shift to remove", LocalText.T(Viewport.WallFamily?.DisplayName ?? "Choose type"))) : (Viewport.Tool == EditorTool.SpecialBrush ? LocalText.Format("Special Brush · {0} · {1} variant(s) · {2}% density",LocalText.T(Viewport.SpecialBrushKind.BrushName()),Viewport.Trees.Count,Viewport.TreeOptions.Density) : (Viewport.IsStampPickingTool ? (Viewport.PickedStamp?.Name ?? ((Viewport.Tool == EditorTool.Clone ? LocalText.T("Choose a placed stamp to clone") : LocalText.T("Choose a placed stamp to move")))) : (StampMode ? (selectedStampName ?? LocalText.T("Choose an object stamp")) : (Viewport.IsCollisionLayer ? LocalText.Format("{0} collision · Paint to apply, erase to remove",LocalText.T(Viewport.Layer == MapLayer.LowCollision ? "Low" : "High")) : (Viewport.SpriteIndex < 0 ? ((PatternPalette ? LocalText.T("Select a terrain set to brush") : LocalText.T("Select a sprite to place"))) : (PatternPalette ? LocalText.Format("Library {0} / Terrain set {1}",Viewport.Library,Viewport.SpriteIndex / AutoTileBrush.BlockSize(AssetLayout(Viewport.Library, Viewport.Layer))) : LocalText.Format("Library {0} / Sprite {1}",Viewport.Library,Viewport.SpriteIndex))))))))); UpdateWorkspace(); UpdateMapBrowserControls(); }

    private async Task Run(Func<Task> action) { if (busy) return; busy = true; UpdateLabels(); try { await action(); } catch (Exception ex) { StudioLog.Error("Editor.Operation",ex,$"Map={mapPath}; Tool={Viewport.Tool}"); MessageBox.Show(this, ex.Message, "Crystal Studio", MessageBoxButton.OK, MessageBoxImage.Warning); Status.Text = LocalText.T("Operation failed"); } finally { busy = false; UpdateLabels(); } }

    private bool ConfirmDiscard() { if (!Viewport.History.Dirty && activeMap?.CategoryDirty != true) return true; var answer = MessageBox.Show(this, "Save your changes before continuing?", activeMap?.IsStamp == true ? "Unsaved stamp" : "Unsaved map", MessageBoxButton.YesNoCancel, MessageBoxImage.Question); if (answer == MessageBoxResult.Cancel) return false; if (answer == MessageBoxResult.Yes) return SaveMap(); return true; }

    private void OnClosing(object? sender, CancelEventArgs e)
    {
        if (busy) { e.Cancel = true; return; }
        foreach (var tab in mapTabs.Items.Cast<TabItem>().ToArray())
        {
            var map = (OpenMapTab)tab.Tag;
            if (!map.CategoryDirty && !(ReferenceEquals(map, activeMap) ? Viewport.History.Dirty : map.State.History.Dirty)) continue;
            mapTabs.SelectedItem = tab;
            if (!ConfirmDiscard()) { e.Cancel = true; return; }
        }
    }

    private void NewClick(object s, RoutedEventArgs e)

    {

        if (busy) return;

        var dialog = new EditorWindow { Title = LocalText.T("New Map"), Width = 400, SizeToContent = SizeToContent.Height, Owner = this, WindowStartupLocation = WindowStartupLocation.CenterOwner, CanResize = false };

        var panel = new StackPanel { Margin = new(24) };
        panel.Children.Add(new TextBlock { Text = LocalText.T("Create a New Crystal Map"), FontSize = 18, Margin = new(0, 0, 0, 12) });
        TextBox width = new TextBox { Text = "256" }, height = new TextBox { Text = "256" };
        panel.Children.Add(new TextBlock { Text = LocalText.T("Quick sizes (width \u00d7 height)"), Margin = new(0, 0, 0, 4) });
        var presets = new WrapPanel { Margin = new(0, 0, 0, 12) };
        foreach (var (w, h) in new[] { (800, 600), (400, 800), (1000, 1000) })
        {
            var preset = new Button { Content = ($"{w} \u00d7 {h}"), Padding = new Thickness(10, 7, 10, 7) };
            preset.Click += (_, _) => { width.Text = w.ToString(); height.Text = h.ToString(); };
            presets.Children.Add(preset);
        }
        panel.Children.Add(presets);
        panel.Children.Add(new TextBlock { Text = LocalText.T("Width in Cells (16\u20134096)")});
        panel.Children.Add(width);
        panel.Children.Add(new TextBlock { Text = LocalText.T("Height in Cells (16\u20134096)")});
        panel.Children.Add(height);
        var create = new Button { Content = LocalText.T("Create Map")};
        panel.Children.Add(create);
        dialog.Content = panel;

        create.Click += (_, _) => { if (!int.TryParse(width.Text, out int w) || !int.TryParse(height.Text, out int h)) { MessageBox.Show(dialog, "Enter dimensions between 16 and 4096."); return; } if (MapDimensions.CreationError(w, h, MapCreationKind.Blank) is { } error) { MessageBox.Show(dialog, LocalText.T(error)); return; } AddMapTab(new(w, h)); Status.Text = LocalText.T("New map created"); dialog.DialogResult = true; }; dialog.ShowDialog();

    }

    private async void OpenClick(object s, RoutedEventArgs e) { if (busy) return; var picker = new OpenFileDialog { Filter = "Mir map (*.map)|*.map", Multiselect = true }; if (picker.ShowDialog(this) == true) await Run(async () => { foreach (var path in picker.FileNames) await OpenMap(path); }); }

    private async Task OpenMap(string path)
    {
        path = Path.GetFullPath(path);
        StudioLog.Info("Map.Open",$"Path={path}; Format={settings.AssetFormat}");
        var existing = mapTabs.Items.Cast<TabItem>().FirstOrDefault(t => string.Equals(((OpenMapTab)t.Tag).Path, path, StringComparison.OrdinalIgnoreCase));
        if (existing != null) { mapTabs.SelectedItem = existing; RememberMap(path); return; }
        Status.Text = LocalText.T("Reading map\u2026");
        var d = await Task.Run(() => MapDocument.Load(path, settings.AssetFormat));
        AddMapTab(d, path);
        try { activeMap!.Recipe = MapCreationStore.LoadRecipe(path, d); }
        catch (Exception exception) { StudioLog.Error("MapCreation.LoadRecipe", exception, $"Path={path}"); }
        StudioLog.Info("Map.Open",$"Loaded; Path={path}; Size={d.Width}x{d.Height}; Stamps={d.PlacedStamps.Count}");
        RememberMap(path);
        Status.Text = (d.MetadataWarning ?? LocalText.Format("Map loaded · {0:N0} remembered stamps", d.PlacedStamps.Count));
    }

    private void IdentifyStampsClick(object s, RoutedEventArgs e)

    {

        if (busy) return;

        var dialog = new StampScanWindow(Viewport.Document, StampCatalog.FindFolder()) { Owner = this };

        if (dialog.ShowDialog() != true || dialog.Result is not StampScanResult result) { Status.Text = LocalText.T("Object identification cancelled \u00b7 Map unchanged"); return; }

        int count = Viewport.Document.RememberIdentifiedStamps(result, Viewport.History);

        Viewport.CancelPickup(); UpdateLabels();

        if (count > 0) SetTool(EditorTool.Pickup);

        Status.Text = (count > 0 ? LocalText.Format("Identified {0:N0} objects · {1:N0}/{2:N0} foreground sprites matched · Save map to keep metadata",count,result.MatchedForegroundImages,result.ForegroundImages) : LocalText.Format("No new complete matches · {0:N0} files checked · {1:N0} unreadable files",result.Files,result.SkippedFiles));

    }

    private void ExitClick(object s, RoutedEventArgs e) => Close();

    private void SaveClick(object s, RoutedEventArgs e) { if (!busy) SaveMap(); }

    private void SaveAsClick(object s, RoutedEventArgs e) { if (!busy) SaveMap(true); }

    private void ExportClassicMapClick(object s, RoutedEventArgs e)
    {
        if (busy || activeMap?.IsStamp == true) return;
        var picker = new SaveFileDialog
        {
            Filter = "Classic Mir2 map (*.map)|*.map",
            FileName = mapPath == null ? "0.map" : Path.GetFileName(mapPath),
            InitialDirectory = mapPath == null ? MapCreationPaths.DefaultDirectory(settings.LibraryFolder) : Path.GetDirectoryName(mapPath),
            DefaultExt = ".map", AddExtension = true, OverwritePrompt = true
        };
        if (picker.ShowDialog(this) != true) return;
        try
        {
            Viewport.Document.SaveClassic(picker.FileName);
            Status.Text = LocalText.Format("Saved classic Mir2 map {0}", Path.GetFileName(picker.FileName));
            StudioLog.Info("Map.ExportClassic", $"Path={picker.FileName}; Size={Viewport.Document.Width}x{Viewport.Document.Height}");
        }
        catch (Exception exception)
        {
            StudioLog.Error("Map.ExportClassic", exception, $"Path={picker.FileName}");
            MessageBox.Show(this, exception.Message, LocalText.T("Export failed"));
        }
    }

    private void DarkModeClick(object s, RoutedEventArgs e) => ThemeService.SetDarkOverride(DarkModeToggle.IsChecked);

    private bool SaveMap(bool saveAs = false)

    {

        if (activeMap?.IsStamp == true) return SaveStampEditor(saveAs);
        string? target = saveAs ? null : mapPath; if (target == null) { var picker = new SaveFileDialog { Filter = "Crystal map (*.map)|*.map", FileName = mapPath == null ? MapCreationPaths.FileName(activeMap?.Name.StartsWith("Untitled ", StringComparison.Ordinal) == false ? activeMap.Name : "Untitled") : Path.GetFileName(mapPath), InitialDirectory = mapPath == null ? MapCreationPaths.DefaultDirectory(settings.LibraryFolder) : Path.GetDirectoryName(mapPath), DefaultExt = ".map", AddExtension = true, OverwritePrompt = true }; if (picker.ShowDialog(this) != true) return false; target = picker.FileName; }

        return SaveMapTo(target);

    }

    private bool SaveMapTo(string target, bool overwrite = true)
    {
        try
        {
            if (mapPath == null && activeMap?.KeepSourceFormat != true) Viewport.Document.SourceFormat = settings.AssetFormat;
            Viewport.Document.PreserveClassicFormat = settings.PreserveOriginalMapFormat;
            if (overwrite) Viewport.Document.Save(target); else Viewport.Document.SaveNew(target);
            mapPath = target; RememberMap(target); Viewport.History.Saved();
            if (activeMap != null) ForgetRecovery(activeMap);
            UpdateLabels(); UpdateMapTabLabels(); Status.Text = LocalText.Format("Saved {0}", Path.GetFileName(target));
            if (activeMap?.Recipe is { } recipe)
            {
                try { activeMap.Recipe = recipe with { AssetFormat = Viewport.Document.SourceFormat }; MapCreationStore.SaveRecipe(target, activeMap.Recipe); }
                catch (Exception exception) { StudioLog.Error("MapCreation.SaveRecipe", exception, $"Path={target}"); Status.Text = LocalText.T("Map saved, but its creation recipe could not be saved."); }
            }
            _ = RefreshSavedMapAsync(target); return true;
        }
        catch (Exception exception)
        { StudioLog.Error("Map.Save", exception, $"Path={target}"); MessageBox.Show(this, exception.Message, "Save failed"); return false; }
    }

    private void SettingsClick(object s, RoutedEventArgs e) { if (!busy) OpenSettings(); }

    private void OpenSettings(bool firstSetup = false)

    {

        var dialog = new SettingsWindow(settingsStore.Load(), async (root, format, ai, preserveClassic) =>

        {

            bool librariesChanged = !string.Equals(root, settings.LibraryFolder, StringComparison.OrdinalIgnoreCase) || format != settings.AssetFormat;
            var next = !string.IsNullOrWhiteSpace(root) && (librariesChanged || assets == null)
                ? await Task.Run(() => AssetStore.Load(root, format)) : null;

            var updated = settingsStore.Load() with { LibraryFolder = string.IsNullOrWhiteSpace(root) ? null : DataPaths.GetClientRoot(root), AssetFormat = format, Ai = ai, PreserveOriginalMapFormat = preserveClassic };

            try { await Task.Run(() => settingsStore.Save(updated)); }

            catch { next?.Dispose(); throw; }

            settings = updated;

            if (next == null) return;

            try { await InitializeWallsAsync(); }
            catch { next.Dispose(); throw; }
            if (closed) { next.Dispose(); return; }
            InstallLibraries(next);
            await RefreshMapBrowserAsync(updated.LibraryFolder);
            await RefreshSetCompositionsAsync(updated.LibraryFolder);

        }, firstSetup)

        { Owner = this };

        dialog.ShowDialog();
        UpdateLanguageChecks();

        if (assets == null && string.IsNullOrWhiteSpace(settings.LibraryFolder))

        {

            Status.Text = LocalText.T("Library setup deferred");

        }

    }

    private async Task LoadLibraries(string root)

    {

        Status.Text = LocalText.T("Loading saved Crystal libraries\u2026");
        var next = await Task.Run(() => AssetStore.Load(root, settings.AssetFormat));
        try { await InitializeWallsAsync(); }
        catch { next.Dispose(); throw; }
        if (closed) { next.Dispose(); return; }
        InstallLibraries(next);
        await RefreshSetCompositionsAsync(root);

    }

    private void AssetsUpdated()
    {
        if (closed || EditorLifetime.IsExiting || Interlocked.Exchange(ref pendingRefresh, 1) != 0) return;
        Dispatcher.UIThread.Post(() => { if (!closed) refresh.Start(); });
    }

    private void AssetError(string message)
    {
        if (closed || EditorLifetime.IsExiting) return;
        Dispatcher.UIThread.Post(() => { if (!closed) Status.Text = LocalText.Format("Asset error: {0}",message); });
    }

    private void InstallLibraries(AssetStore next)

    {

        assets?.Dispose(); assets = next; next.Updated += AssetsUpdated; next.Error += AssetError; Viewport.Assets = next; Viewport.Town = null; Viewport.RefreshAssets();

        LibraryList.ItemsSource = next.Libraries.OrderBy(k => k.Key).Select(k => new LibraryChoice(k.Key, $"{k.Key:D3}   {Path.GetFileNameWithoutExtension(k.Value.Path)}")).ToArray(); LibraryList.SelectedIndex = 0; _ = DiscoverStampsAsync(); Status.Text = LocalText.Format("{0} libraries indexed",next.Libraries.Count);

    }

    private async void StampClick(object s, RoutedEventArgs e) { var picker = new OpenFileDialog { Filter = "Crystal object (*.X)|*.X" }; if (picker.ShowDialog(this) != true) return; await Run(async () => { Viewport.Stamp = await Task.Run(() => CellCodec.LoadStamp(picker.FileName, settings.AssetFormat)); selectedStampPath = picker.FileName; selectedStampName = Path.GetFileNameWithoutExtension(picker.FileName); Viewport.StampName = selectedStampName; selectedStampPreview = assets == null ? null : StampPreview.Create(Viewport.Stamp, assets); SetTool(EditorTool.Stamp); RememberActiveAsset(); Viewport.InvalidateVisual(); Status.Text = LocalText.Format("Stamp: {0} • {1} cells",Path.GetFileName(picker.FileName),Viewport.Stamp.Count); }); }

    private void ExportClick(object s, RoutedEventArgs e)

    {

        if (Viewport.Selection is not Rect r) { Status.Text = LocalText.T("Use Select (M) to mark the object area first"); return; }

        var picker = new SaveFileDialog { Filter = "Crystal object (*.X)|*.X", FileName = "Object.X" }; if (picker.ShowDialog(this) != true) return;

        try { List<StampCell> cells = []; for (int y = (int)r.Y; y < r.Bottom; y++) for (int x = (int)r.X; x < r.Right; x++) { var c = Viewport.Document.Get(x, y); if (!c.Empty) cells.Add(new(x - (int)r.X - (int)r.Width / 2, y - (int)r.Y - (int)r.Height / 2, c)); } CellCodec.SaveStamp(picker.FileName, cells); Status.Text = LocalText.Format("Exported {0} object cells",cells.Count); } catch (Exception ex) { StudioLog.Error("Stamp.Export",ex,$"Path={picker.FileName}"); MessageBox.Show(this, ex.Message, "Export failed"); }

    }

    private void SetTool(EditorTool tool) { if (tool == EditorTool.Wall) { Viewport.Layer = MapLayer.Objects; LayerList.SelectedIndex = (int)MapLayer.Objects; Status.Text = LocalText.T("Draw connected wall sections \u00b7 Shift or Remove deletes and reconnects"); } if (tool == EditorTool.Pickup) Status.Text = LocalText.T("Click a placed stamp to pick it up"); else if (tool == EditorTool.Clone) Status.Text = LocalText.T("Click a placed stamp to clone it"); bool changingBrowser = StampMode != (tool == EditorTool.Stamp); Viewport.Tool = tool; if (changingBrowser) { searchDelay.Stop(); AssetSearch.Clear(); } page = 0; BuildPage(); BrushSize.IsEnabled = tool != EditorTool.Pencil; UpdateLabels(); Viewport.InvalidateVisual(); }

    private void ToolClick(object s, RoutedEventArgs e)
    {
        var tool = Enum.Parse<EditorTool>((string)((Control)s).Tag);
        SetTool(tool);
        if (tool == EditorTool.Eyedropper) Status.Text = LocalText.T("Click a ground or middle tile to select its source artwork.");
        else if (tool == EditorTool.Pan) Status.Text = LocalText.T("Drag with the left mouse button to pan. Middle drag and Space + drag also work.");
    }

    private void LayerChanged(object s, SelectionChangedEventArgs e)
    {
        if (!initialized || Viewport == null || restoringAsset) return;
        Viewport.Layer = (MapLayer)LayerList.SelectedIndex; Viewport.SpriteIndex = -1; page = 0;
        if (Viewport.Tool == EditorTool.Eyedropper && Viewport.Layer is not (MapLayer.Ground or MapLayer.Middle))
            SetTool(EditorTool.Pencil);
        if (Viewport.Tool is EditorTool.SpecialBrush or EditorTool.Wall or EditorTool.TownStamp && Viewport.Layer != MapLayer.Objects || Viewport.IsCollisionLayer && Viewport.Tool is EditorTool.Stamp or EditorTool.Pickup or EditorTool.Clone)
            SetTool(EditorTool.Pencil);
        SelectLibraryForLayer(); BuildPage(); UpdateLabels(); Viewport.InvalidateVisual();
    }

    private void BrushChanged(object s, RangeBaseValueChangedEventArgs e) { if (Viewport != null) { Viewport.BrushSize = (int)e.NewValue; if (BrushSizeLabel != null) BrushSizeLabel.Text = ($"{Viewport.BrushSize}\u00d7{Viewport.BrushSize}"); Viewport.InvalidateVisual(); UpdateWorkspace(); } }

    private void ViewChanged(object s, RoutedEventArgs e)
    {
        if (!initialized || Viewport == null || ObjectsVisible == null) return;
        Viewport.GridVisible = GridToggle.IsChecked == true;
        Viewport.LowCollisionVisible = LowCollisionVisible.IsChecked == true;
        Viewport.HighCollisionVisible = HighCollisionVisible.IsChecked == true;
        Viewport.VisibleLayers[0] = GroundVisible.IsChecked == true;
        Viewport.VisibleLayers[1] = MiddleVisible.IsChecked == true;
        Viewport.VisibleLayers[2] = ObjectsVisible.IsChecked == true;
        Viewport.LightCellsVisible = LightCellsVisible.IsChecked == true;
        Viewport.DoorCellsVisible = DoorCellsVisible.IsChecked == true;
        Viewport.DoorsOpen = DoorsOpenToggle.IsChecked == true;
        Viewport.InvalidateVisual();
    }

    private void LightingClick(object s, RoutedEventArgs e)
    {
        if (s is not MenuItem item || !Enum.TryParse<MapLighting>(item.Tag?.ToString(), out var lighting)) return;
        Viewport.Lighting = lighting;
        foreach (MenuItem choice in MapLightingMenu.Items)
            choice.IsChecked = choice == item;
        Viewport.InvalidateVisual();
    }

    private void UndoClick(object s, RoutedEventArgs e) => Viewport.Undo();

    private void RedoClick(object s, RoutedEventArgs e) => Viewport.Redo();

    private void FillClick(object s, RoutedEventArgs e) { if (Viewport.Selection == null) { Status.Text = LocalText.T("Select an area (M), choose a sprite, then fill"); return; } Viewport.FillSelection(); }

    private void InitializeCellMenu()

    {

        var menu = new ContextMenu();

        var cellProperties = new MenuItem { Header = LocalText.T("Cell Properties\u2026")};

        var selectionProperties = new MenuItem { Header = LocalText.T("Selection Properties\u2026")};

        menu.Items.Add(cellProperties); menu.Items.Add(selectionProperties);

        var copy = new MenuItem { Header = LocalText.T("Copy Visible Layers"), InputGesture = KeyGesture.Parse("Ctrl+C") };

        var cut = new MenuItem { Header = LocalText.T("Cut Visible Layers"), InputGesture = KeyGesture.Parse("Ctrl+X") };

        var paste = new MenuItem { Header = LocalText.T("Paste Region\u2026"), InputGesture = KeyGesture.Parse("Ctrl+V") };

        menu.Items.Add(new Separator()); menu.Items.Add(copy); menu.Items.Add(cut); menu.Items.Add(paste);

        cut.Click += (_, _) => Viewport.CutSelection();

        copy.Click += (_, _) => Viewport.CopySelection();

        paste.Click += (_, _) => Viewport.BeginPaste();

        (int X, int Y) clicked = default;

        Viewport.PointerPressed += (_, e) => { if (e.GetCurrentPoint(Viewport).Properties.IsRightButtonPressed) clicked = Viewport.CellAt(e.GetPosition(Viewport)); };

        menu.Opening += (_, e) =>
        {

            if (busy || !Viewport.Document.Contains(clicked.X, clicked.Y)) { e.Cancel = true; return; }

            copy.IsEnabled = cut.IsEnabled = Viewport.Selection.HasValue; paste.IsEnabled = Viewport.CanPasteRegion;

            selectionProperties.IsVisible = Viewport.Selection.HasValue ? true : false;

        };

        cellProperties.Click += (_, _) => OpenCellProperties(clicked, new Rect(clicked.X, clicked.Y, 1, 1));

        selectionProperties.Click += (_, _) => { if (Viewport.Selection is Rect area) OpenCellProperties(((int)area.X, (int)area.Y), area); };

        Viewport.ContextMenu = menu;

    }

    private void OpenCellProperties((int X, int Y) point, Rect area)

    {

        if (busy || !Viewport.Document.Contains(point.X, point.Y)) return;
        if (Viewport.IsLayerLocked(MapLayer.Objects)) { Status.Text = LocalText.T("This layer is locked."); return; }

        var dialog = new EditorWindow { Title = LocalText.T("Cell Properties"), Width = 340, SizeToContent = SizeToContent.Height, Owner = this, WindowStartupLocation = WindowStartupLocation.CenterOwner, CanResize = false }; var panel = new StackPanel { Margin = new(24) }; dialog.Content = panel;

        panel.Children.Add(new TextBlock { Text = (area.Width == 1 && area.Height == 1 ? LocalText.Format("Cell {0}, {1}",point.X,point.Y) : LocalText.T("Apply to Selected Cells")), FontSize = 18, Margin = new(0, 0, 0, 14) });

        var initialValues = new Dictionary<TextBox, string>();
        TextBox Field(string label, Func<Cell, byte> read)
        {
            panel.Children.Add(new TextBlock { Text = LocalText.T(label) });
            byte? common = Viewport.CommonMetadata(area, read);
            var box = new TextBox { Text = common?.ToString() ?? "", Watermark = LocalText.T("Mixed — leave unchanged") };
            initialValues[box] = box.Text ?? "";
            panel.Children.Add(box);
            return box;
        }
        var light = Field("Light (0\u2013255)", cell => cell.Light);
        var door = Field("Door Index (0\u2013255)", cell => cell.Door);
        var offset = Field("Door Offset (0\u2013255)", cell => cell.DoorOffset);
        var apply = new Button { Content = LocalText.T("Apply Properties") }; panel.Children.Add(apply);
        apply.Click += (_, _) =>
        {
            bool Parse(TextBox box, out byte? value)
            {
                value = null;
                if (string.IsNullOrWhiteSpace(box.Text)) return true;
                if (!byte.TryParse(box.Text, out var parsed)) return false;
                value = parsed; return true;
            }
            if (!Parse(light, out var l) || !Parse(door, out var d) || !Parse(offset, out var o))
            { MessageBox.Show(dialog, "One or more values are outside the supported range."); return; }
            // Unchanged fields must not flatten mixed values in a selection.
            Viewport.ApplyMetadata(light.Text == initialValues[light] ? null : l,
                door.Text == initialValues[door] ? null : d,
                offset.Text == initialValues[offset] ? null : o, area);
            dialog.DialogResult = true;
        };
        dialog.ShowDialog();

    }

    private void FitClick(object s, RoutedEventArgs e) => Viewport.Fit();

    private void SelectLibraryForLayer()

    {

        if (assets == null || Viewport.IsCollisionLayer) return;

        bool Matches(int id) => Viewport.Layer == MapLayer.Objects ? Path.GetFileNameWithoutExtension(assets.Libraries[id].Path).Contains("object", StringComparison.OrdinalIgnoreCase) : AssetLayout(id, Viewport.Layer) != TilePatternLayout.None;

        if (assets.Libraries.ContainsKey(Viewport.Library) && Matches(Viewport.Library)) return;

        var preferred = assets.Libraries.Keys.Order().Where(Matches).Cast<int?>().FirstOrDefault();

        if (preferred.HasValue) LibraryList.SelectedItem = LibraryList.Items.OfType<LibraryChoice>().FirstOrDefault(l => l.Id == preferred.Value);

    }

    private void LibraryChanged(object s, SelectionChangedEventArgs e) { if (restoringAsset) return; page = 0; Viewport.SpriteIndex = -1; if (LibraryList.SelectedItem is LibraryChoice l) Viewport.Library = l.Id; BuildPage(); }

    private void SelectPickedTile(int library, int index, MapLayer layer)
    {
        if (assets?.Libraries.TryGetValue(library, out var atlas) != true || index < 0 || index >= atlas.Count)
        {
            Status.Text = LocalText.T("Tile library or image is unavailable");
            return;
        }

        restoringAsset = true;
        try
        {
            LayerList.SelectedIndex = (int)layer;
            Viewport.Layer = layer;
            Viewport.Library = library;
            Viewport.SpriteIndex = index;
            LibraryList.SelectedItem = LibraryList.Items.OfType<LibraryChoice>().FirstOrDefault(l => l.Id == library);
        }
        finally { restoringAsset = false; }

        // A search filter can hide the sampled tile; clear it so the reverse lookup
        // always reveals and selects the matching palette item.
        searchDelay.Stop();
        // Avalonia leaves Text null before input and after Clear().
        if (!string.IsNullOrEmpty(AssetSearch.Text)) { AssetSearch.Clear(); searchDelay.Stop(); }
        var layout = PatternPalette ? AssetLayout(library, layer) : TilePatternLayout.None;
        var paletteAsset = AssetReference.Tile(atlas.Path, library, index, layer, layout);
        var visibleIndices = FilteredIndices(atlas);
        var targetIndex = visibleIndices.IndexOf(paletteAsset.Index);
        page = targetIndex >= 0 ? targetIndex / PageSize : 0;
        BuildPage();
        var selected = choices.FirstOrDefault(c => c.Asset is AssetReference asset && asset.SameAsset(paletteAsset));
        if (selected != null)
        {
            AssetList.SelectedItem = selected;
            AssetList.ScrollIntoView(selected);
        }
        RememberActiveAsset();
        UpdateLabels();
        Viewport.InvalidateVisual();
        Status.Text = LocalText.Format("Selected tile · Lib {0} · Sprite {1}", library, index);
    }

    private void SearchChanged(object s, TextChangedEventArgs e) { page = 0; if (initialized) { searchDelay.Stop(); searchDelay.Start(); } }

    private void PreviousPage(object s, RoutedEventArgs e) { if (CategoryPalette) { ChangeCategoryPage(-1); return; } page = Math.Max(0, page - 1); BuildPage(); }

    private void NextPage(object s, RoutedEventArgs e) { if (CategoryPalette) { ChangeCategoryPage(1); return; } if ((page + 1) * PageSize < filteredCount) { page++; BuildPage(); } }

    private TilePatternLayout AssetLayout(int library, MapLayer layer) =>
        AutoTileBrush.Layout(library, layer, assets?.Format ?? AssetSourceFormat.Default);

    private bool PatternPalette => Viewport.Tool == EditorTool.Brush && AssetLayout(Viewport.Library, Viewport.Layer) != TilePatternLayout.None;

    private List<int> FilteredIndices(IAssetLibrary lib)

    {

        var layout = AssetLayout(Viewport.Library, Viewport.Layer); int block = AutoTileBrush.BlockSize(layout); IEnumerable<int> indices;

        if (PatternPalette) { indices = Enumerable.Range(0, lib.Count / block).Select(i => i * block); if (layout == TilePatternLayout.Mir3) indices = indices.Concat(Enumerable.Range(0, lib.Count / block).Select(i => i * block + 5)); }

        else indices = Enumerable.Range(0, Math.Min(lib.Count, LayerList.SelectedIndex == 2 ? 32767 : 65535));

        string query = AssetSearch.Text?.Trim() ?? "";
        return indices.Where(i => query.Length == 0 || (PatternPalette ? (i / block).ToString() : i.ToString()).Contains(query, StringComparison.OrdinalIgnoreCase)).ToList();

    }

    private void BuildPage()
    {
        if (Viewport.Tool is EditorTool.Wall or EditorTool.TownStamp) { pageVersion++; choices.Clear(); filteredCount = 0; UpdateWorkspace(); return; }
        if (CategoryPalette) { BuildCategoryBrowser(); return; }
        if (Viewport.IsCollisionLayer && !StampMode && !SavedPalette)
        { pageVersion++; choices.Clear(); filteredCount = 0; UpdateWorkspace(); return; }
        if (SavedPalette) { BuildSavedPage(); return; }
        if (StampMode) { BuildStampPage(); return; }
        pageVersion++; choices.Clear(); filteredCount = 0;
        if (assets?.Libraries.TryGetValue(Viewport.Library, out var lib) != true) { UpdateWorkspace(); return; }
        var indices = FilteredIndices(lib!); filteredCount = indices.Count;
        page = Math.Clamp(page, 0, Math.Max(0, (filteredCount - 1) / PageSize));
        foreach (int i in indices.Skip(page * PageSize).Take(PageSize))
            choices.Add(TileChoice(AssetReference.Tile(lib!.Path, Viewport.Library, i, Viewport.Layer,
                PatternPalette ? AssetLayout(Viewport.Library, Viewport.Layer) : TilePatternLayout.None)));
        PageLabel.Text = (indices.Count == 0 ? LocalText.T("No Matching Assets") : LocalText.Format("Page {0} of {1}",page + 1,Math.Max(1, (indices.Count + PageSize - 1) / PageSize)));
        UpdateWorkspace();
    }

    private PlacedStamp? previewStamp;
    private AssetStore? previewAssets;
    private RasterImage? pickupPreview;
    private RasterImage? ActivePreview()
    {
        if (Viewport.Tool == EditorTool.TownStamp) return assets == null || Viewport.Town == null ? null : StampPreview.Create(Viewport.Town.Cells, assets);
        if (Viewport.Tool == EditorTool.Wall) return assets == null || Viewport.WallFamily == null ? null : StampPreview.Create(Viewport.WallFamily.BrushPreview(), assets);
        if (Viewport.Tool == EditorTool.SpecialBrush) return assets == null || Viewport.Trees.Count == 0 ? null : StampPreview.Create(Viewport.Trees[0].Cells, assets);
        if (!Viewport.IsStampPickingTool) return StampMode ? selectedStampPreview : ActiveTilePreview();
        var stamp = Viewport.PickedStamp;
        if (!ReferenceEquals(stamp, previewStamp) || !ReferenceEquals(assets, previewAssets))
        { previewStamp = stamp; previewAssets = assets; pickupPreview = null; }
        if (stamp == null || assets == null) return null;
        return pickupPreview ??= StampPreview.Create(stamp.Cells, assets);
    }

    private void RefreshThumbnails() => RefreshPaletteThumbnails();

    private void AssetChanged(object s, SelectionChangedEventArgs e)
    {
        if (AssetList.SelectedItem is not AssetChoice a) return;
        if (a.StampPath != null) { if (a.StampCells != null) SelectStamp(a); else Status.Text = (a.LoadError ?? LocalText.T("Loading stamp\u2026")); return; }
        if (SavedPalette) { RestoreTile(a); return; }
        Viewport.SpriteIndex = a.Index;
        if (Viewport.Tool != EditorTool.Pencil && Viewport.Tool != EditorTool.Brush && Viewport.Tool != EditorTool.Bucket)
        {
            // The raw-sprite palette is already correct. Rebuilding it through SetTool
            // here clears the items while Avalonia is still committing their selection.
            Viewport.Tool = EditorTool.Pencil;
            BrushSize.IsEnabled = false;
        }
        RememberActiveAsset(); UpdateLabels(); Viewport.InvalidateVisual();
    }

    private void DeleteStampClick(object s, RoutedEventArgs e) => Viewport.DeletePicked();

    private void CancelPickupClick(object s, RoutedEventArgs e) { Viewport.CancelPickup(); UpdateLabels(); }

    private void MovePaletteSelection(int direction)
    {
        if (CategoryPalette) return;
        bool pendingSearch = searchDelay.IsEnabled;
        searchDelay.Stop();
        int current, count;
        if (SavedPalette)
        {
            if (pendingSearch) BuildPage();
            // Keep keyboard navigation in the displayed order while selection
            // updates the persisted recent list.
            var filtered = savedBrowserAssets;
            var currentAsset = (AssetList.SelectedItem as AssetChoice)?.Asset ?? ActiveAssetReference();
            current = currentAsset == null ? -1 : filtered.FindIndex(currentAsset.SameAsset);
            count = filtered.Count;
        }
        else if (StampMode)
        {
            string query = AssetSearch.Text?.Trim() ?? "";
            var filtered = stampEntries.Where(entry => query.Length == 0 || entry.RelativePath.Contains(query, StringComparison.OrdinalIgnoreCase)).ToList();
            string? path = (AssetList.SelectedItem as AssetChoice)?.StampPath ?? selectedStampPath;
            current = filtered.FindIndex(entry => entry.FilePath == path);
            count = filtered.Count;
        }
        else
        {
            if (assets?.Libraries.TryGetValue(Viewport.Library, out var library) != true) return;
            var indices = FilteredIndices(library!);
            current = indices.IndexOf(Viewport.SpriteIndex);
            count = indices.Count;
        }
        if (count == 0) return;
        int target = current < 0 ? Math.Min(page * PageSize, count - 1) : Math.Clamp(current + direction, 0, count - 1);
        page = target / PageSize;
        // Rebuild to apply any pending search and reveal items across page boundaries.
        if (SavedPalette) BuildSavedPage(savedBrowserAssets); else BuildPage();
        AssetList.SelectedIndex = target % PageSize;
        if (AssetList.SelectedItem is AssetChoice choice) AssetList.ScrollIntoView(choice);
    }
    private void KeyDownHandler(object s, KeyEventArgs e)

    {
        if (!busy && FocusManager?.GetFocusedElement() is not TextBox && e.Key == Key.N && e.KeyModifiers == (KeyModifiers.Control | KeyModifiers.Alt))
        { QuickCreateClick(s, e); e.Handled = true; return; }

        if (FocusManager?.GetFocusedElement() is TextBox or MenuItem || mapFileList.IsKeyboardFocusWithin || InputState.Modifiers.HasFlag(KeyModifiers.Alt) || busy) return; bool ctrl = InputState.Modifiers.HasFlag(KeyModifiers.Control);

        if (ctrl) { if (e.Key == Key.C) { Viewport.CopySelection(); e.Handled = true; } else if (e.Key == Key.X) { Viewport.CutSelection(); e.Handled = true; } else if (e.Key == Key.V) { Viewport.BeginPaste(); e.Handled = true; } else if (e.Key == Key.S) { SaveMap(InputState.Modifiers.HasFlag(KeyModifiers.Shift)); e.Handled = true; } else if (e.Key == Key.Z) { if (InputState.Modifiers.HasFlag(KeyModifiers.Shift)) Viewport.Redo(); else Viewport.Undo(); e.Handled = true; } else if (e.Key == Key.Y) { Viewport.Redo(); e.Handled = true; } else if (e.Key == Key.N) { if (InputState.Modifiers.HasFlag(KeyModifiers.Shift)) NewStampClick(s, e); else NewClick(s, e); e.Handled = true; } else if (e.Key == Key.O) { OpenClick(s, e); e.Handled = true; } return; }

        if (e.Key is Key.W or Key.A or Key.S or Key.D)
        {
            if (FocusManager?.GetFocusedElement() is ComboBox || LibraryList.IsDropDownOpen) return;
            Viewport.PanBy(e.Key switch { Key.W => new Vector(0, 48), Key.A => new Vector(48, 0), Key.S => new Vector(0, -48), _ => new Vector(-48, 0) });
            e.Handled = true;
            return;
        }

        if (CategoryPalette && CategoryTabs.IsKeyboardFocusWithin && e.Key is Key.Left or Key.Right or Key.Up or Key.Down) return;
        if (e.Key is Key.Left or Key.Right && FocusManager?.GetFocusedElement() is not ComboBox && !LibraryList.IsDropDownOpen) { MovePaletteSelection(e.Key == Key.Left ? -1 : 1); e.Handled = true; return; }

        EditorTool? tool = e.Key switch { Key.B => EditorTool.Brush, Key.P => EditorTool.Pencil, Key.F => EditorTool.Bucket, Key.E => EditorTool.Eraser, Key.M => EditorTool.Select, Key.O => EditorTool.Stamp, Key.V => EditorTool.Pickup, Key.K => EditorTool.Clone, _ => null }; if (tool.HasValue) { SetTool(tool.Value); e.Handled = true; }
        if (e.Key == Key.T) { SpecialBrushClick(s, e); e.Handled = true; }
        if (e.Key == Key.H) { WallBrushClick(s, e); e.Handled = true; }

        if (e.Key == Key.F1) { ShortcutsClick(s, e); e.Handled = true; }

        if (e.Key == Key.G) { GridToggle.IsChecked = GridToggle.IsChecked != true; ViewChanged(s, e); e.Handled = true; }

        if (e.Key == Key.C) { bool show = !Viewport.CollisionVisible; LowCollisionVisible.IsChecked = show; HighCollisionVisible.IsChecked = show; e.Handled = true; }

        if (e.Key == Key.OemOpenBrackets || e.Key == Key.OemCloseBrackets) { BrushSize.Value = Math.Clamp(BrushSize.Value + (e.Key == Key.OemOpenBrackets ? -2 : 2), 3, 31); e.Handled = true; }

        if (e.Key == Key.Delete) { if (Viewport.Tool == EditorTool.Pickup) Viewport.DeletePicked(); else if (Viewport.Tool != EditorTool.Clone) Viewport.ClearSelection(); e.Handled = true; }

        if (e.Key == Key.Escape) { Viewport.CancelInteraction(); UpdateLabels(); e.Handled = true; }

        Viewport.InvalidateVisual();

    }

    private void UpdateWorkspace()

    {

        if (!initialized || ToolPanel == null) return;

        foreach (var button in ToolPanel.Children.OfType<RadioButton>())

            button.IsChecked = button.Tag?.ToString() == Viewport.Tool.ToString();

        PickupActions.IsVisible = Viewport.IsStampPickingTool ? true : false;

        TownControls.IsVisible = Viewport.Tool == EditorTool.TownStamp ? true : false;
        TownBrushLabel.Text = (Viewport.Town == null ? LocalText.T("Choose a Town") : ($"{Viewport.Town.Family.Name} \u00b7 {Viewport.Town.Options.Size}\u00d7{Viewport.Town.Options.Size}"));
        TownSettingsButton.IsEnabled = TownStampMenuButton.IsEnabled = !busy;
        WallControls.IsVisible = Viewport.Tool == EditorTool.Wall ? true : false;
        WallSettingsButton.IsEnabled = WallEraseToggle.IsEnabled = !busy;
        WallBrushLabel.Text = (Viewport.WallFamily?.DisplayName ?? LocalText.T("Choose Type"));
        SizeControls.IsVisible = Viewport.IsStampPickingTool || Viewport.Tool is EditorTool.Wall or EditorTool.TownStamp or EditorTool.Pan or EditorTool.Eyedropper ? false : true;
        SpecialBrushSettingsButton.IsVisible = Viewport.Tool == EditorTool.SpecialBrush ? true : false;

        DeleteStampButton.IsVisible = Viewport.Tool == EditorTool.Clone ? false : true;
        DeleteStampButton.IsEnabled = CancelPickupButton.IsEnabled = !busy && Viewport.PickedStampId != null;
        ToolTip.SetTip(CancelPickupButton, (Viewport.Tool == EditorTool.Clone ? LocalText.T("Cancel Clone \u00b7 Escape") : LocalText.T("Cancel Pickup \u00b7 Escape")));

        SaveButton.Header = (activeMap?.IsStamp == true ? LocalText.T("_Save Stamp") : LocalText.T("_Save Map"));
        this.Title = LocalText.Format("{0}{1} • {2} × {3} cells — Crystal Studio",(mapPath == null ? LocalText.T("Untitled") : Path.GetFileName(mapPath)),(Viewport.History.Dirty || activeMap?.CategoryDirty == true ? " *" : ""),Viewport.Document.Width,Viewport.Document.Height);

        GenerateProvinceButton.IsEnabled = GenerateTempleButton.IsEnabled = GenerateTerrainButton.IsEnabled = IdentifyButton.IsEnabled = NewButton.IsEnabled = NewStampButton.IsEnabled = OpenButton.IsEnabled = OpenRecentButton.IsEnabled = ImportButton.IsEnabled = SaveButton.IsEnabled = SaveAsButton.IsEnabled = !busy;

        ToolPanel.IsEnabled = LibraryList.IsEnabled = AssetSearch.IsEnabled = !busy;
        AssetList.IsEnabled = !busy;
        mapTabs.IsEnabled = !busy;
        if (paletteTabs != null) paletteTabs.IsEnabled = !busy;

        Viewport.IsHitTestVisible = !busy;
        if (this.FindControl<Control>("Minimap") is MapMinimap minimap) minimap.IsEnabled = !busy;

        BusyProgress.IsVisible = busy ? true : false;

        UndoButton.IsEnabled = !busy && Viewport.History.CanUndo;

        RedoButton.IsEnabled = !busy && Viewport.History.CanRedo;

        ExportButton.IsEnabled = !busy && Viewport.Selection.HasValue;
        ExportMinimapButton.IsEnabled = !busy;
        bool canCopy = !busy && Viewport.Selection.HasValue && (Viewport.VisibleLayers.Any(v => v) || Viewport.CollisionVisible);
        CopyRegionMenuButton.IsEnabled = canCopy;
        CutRegionMenuButton.IsEnabled = canCopy && !Viewport.AnyLayerLocked;
        PasteRegionMenuButton.IsEnabled = !busy && Viewport.CanPasteRegion && !Viewport.AnyLayerLocked;
        EyedropperToolButton.IsEnabled = !busy && Viewport.Layer is MapLayer.Ground or MapLayer.Middle;
        CropButton.IsEnabled = CropMenuButton.IsEnabled = !busy && activeMap?.IsStamp != true &&
            Viewport.Selection.HasValue && !Viewport.AnyLayerLocked;
        ExpandMapMenuButton.IsEnabled = !busy && activeMap?.IsStamp != true && !Viewport.AnyLayerLocked;

        FillButton.IsEnabled = !busy && Viewport.Selection.HasValue && (Viewport.SpriteIndex >= 0 || Viewport.IsCollisionLayer);

        BrushSize.IsEnabled = !busy && Viewport.Tool is EditorTool.Brush or EditorTool.Eraser or EditorTool.SpecialBrush;

        BrushSizeLabel.Text = (Viewport.Tool == EditorTool.Bucket ? LocalText.T("Fill") : (Viewport.Tool == EditorTool.Pencil ? ("1\u00d71") : ($"{Viewport.BrushSize}\u00d7{Viewport.BrushSize}")));

        Viewport.Cursor = Viewport.Tool == EditorTool.TownStamp || Viewport.Tool == EditorTool.Wall || Viewport.Tool == EditorTool.Select || Viewport.Tool == EditorTool.Brush || Viewport.Tool == EditorTool.SpecialBrush || Viewport.Tool == EditorTool.Pencil || Viewport.Tool == EditorTool.Bucket ? EditorCursors.Cross : EditorCursors.Arrow;
        if (Viewport.Tool == EditorTool.Pan) Viewport.Cursor = EditorCursors.Hand;
        else if (Viewport.Tool == EditorTool.Eyedropper) Viewport.Cursor = EditorCursors.Cross;

        SelectedPreviewCaption.Text = (Viewport.IsStampPickingTool ? LocalText.T("Placed Stamp") : LocalText.T("Active Asset"));

        AssetTitle.Text = (Viewport.Tool == EditorTool.TownStamp ? LocalText.T("Town Stamp") : (Viewport.Tool == EditorTool.Wall ? LocalText.T("Wall Brush") : (SavedPalette ? (FavouritesPalette ? LocalText.T("Favourites") : LocalText.T("Recently Used")) : (StampMode ? LocalText.T("Object Stamps") : (PatternPalette ? LocalText.T("Terrain Patterns") : LocalText.T("Tile & Object Library"))))));

        AssetCount.Text = ($"{filteredCount:N0} {(SavedPalette ? "assets" : StampMode ? "stamps" : PatternPalette ? "sets" : "sprites")}");

        PreviousButton.IsEnabled = !busy && page > 0;

        NextButton.IsEnabled = !busy && (page + 1) * PageSize < filteredCount;

        if (assets == null && !StampMode && !SavedPalette) PageLabel.Text = LocalText.T("No Library Loaded");

        LibraryList.IsVisible = StampMode || SavedPalette || Viewport.IsCollisionLayer ? false : true;

        StampFolderHint.IsVisible = StampMode && !SavedPalette ? true : false;

        StampFolderLabel.Text = (stampFolder ?? LocalText.T("Objects Folder Not Found")); ToolTip.SetTip(StampFolderLabel, (stampFolder));

        (AssetSearch).Tag = LocalText.T(SavedPalette ? "Search Saved Assets\u2026" : StampMode ? "Search Stamps by Name\u2026" : "Search Pattern or Sprite ID\u2026");
        ToolTip.SetTip(AssetSearch, (SavedPalette ? LocalText.T("Search Names, Layers, Libraries or IDs") : (StampMode ? LocalText.T("Search Stamps by Name") : LocalText.T("Filter Assets by Numeric ID"))));
        EmptyAction.IsVisible = true;

        AssetEmpty.IsVisible = choices.Count == 0 ? true : false;
        var activeAsset = ActiveAssetReference();
        SelectedPinButton.IsEnabled = !busy && activeAsset != null;
        SelectedPinButton.Content = (activeAsset != null && assetHistory.IsFavourite(activeAsset) ? LocalText.T("\u2605 Unpin") : LocalText.T("\u2606 Pin to Favourites"));

        if (Viewport.Tool == EditorTool.TownStamp)
        {
            PageLabel.Text = LocalText.T("Town Settings");
            EmptyTitle.Text = (Viewport.Town?.Family.Name ?? LocalText.T("Choose a Town"));
            EmptyDescription.Text = LocalText.T("Click a clear area to place the complete town.\nTown settings changes the type, size and seed.");
            EmptyAction.IsVisible = false;
        }
        else if (Viewport.Tool == EditorTool.Wall)
        {
            PageLabel.Text = LocalText.T("Wall Settings \u00b7 H");
            EmptyTitle.Text = (Viewport.WallFamily?.Name ?? LocalText.T("Wall Catalogue Unavailable"));
            EmptyDescription.Text = (Viewport.WallCatalogueError ?? string.Concat(((Viewport.WallFamily?.DrawInterior == true ? LocalText.T("Draw the room interior; walls surround the open floor.") : LocalText.T("Draw the solid wall area; walls surround its edges."))), LocalText.T("\nShift or Remove deletes sections and reconnects neighbours.")));
            EmptyAction.IsVisible = false;
        }
        else if (Viewport.IsCollisionLayer && !StampMode && !SavedPalette)
        {
            PageLabel.Text = LocalText.T("No Asset Needed");
            EmptyTitle.Text = (Viewport.Layer == MapLayer.LowCollision ? LocalText.T("Low Collisions") : LocalText.T("High Collisions"));
            EmptyDescription.Text = LocalText.T("Paint to apply. Fill (F) floods connected clear cells.\nUse Eraser or selection Delete to remove.");
            EmptyAction.IsVisible = false;
        }
        else if (SavedPalette)
        {
            bool searching = !string.IsNullOrWhiteSpace(AssetSearch.Text);
            EmptyTitle.Text = (searching ? LocalText.T("No Matching Assets") : (FavouritesPalette ? LocalText.T("Pin Your Favourite Assets") : LocalText.T("No Recent Assets Yet")));
            EmptyDescription.Text = (searching ? LocalText.T("Try another name, layer or ID.") : (FavouritesPalette ? LocalText.T("Click the star on a terrain tile or stamp\nto keep it here.") : LocalText.T("Assets you select or paint with appear here,\nwith the most recent first.")));
            EmptyAction.Content = LocalText.T("Clear Search");
            EmptyAction.IsVisible = searching ? true : false;
        }
        else if (StampMode)

        {

            EmptyTitle.Text = (stampIndexLoading ? LocalText.T("Finding Object Stamps\u2026") : (stampFolder == null ? LocalText.T("Objects Folder Not Found") : LocalText.T("No Matching Stamps")));

            EmptyDescription.Text = (stampFolder == null ? LocalText.T("Place your .X files in the Objects folder at the editor root, or import a stamp.") : LocalText.T("Try another name or import a .X stamp."));

            EmptyAction.Content = (string.IsNullOrWhiteSpace(AssetSearch.Text) ? LocalText.T("Import .X Stamp") : LocalText.T("Clear Search"));

        }

        else if (assets == null)

        {

            EmptyTitle.Text = LocalText.T("Your Assets Live Here");

            EmptyDescription.Text = LocalText.T("Choose your Crystal library folder once.\nIt will load automatically next time.");

            EmptyAction.Content = LocalText.T("Set Library Folder");

        }

        else if (!string.IsNullOrWhiteSpace(AssetSearch.Text))

        {

            EmptyTitle.Text = LocalText.T("No Matching Assets"); EmptyDescription.Text = LocalText.T("Try a different pattern or sprite ID."); EmptyAction.Content = LocalText.T("Clear Search");

        }

        else

        {

            EmptyTitle.Text = LocalText.T("No Patterns in This Library"); EmptyDescription.Text = LocalText.T("Switch to Pencil to browse individual tiles."); EmptyAction.Content = LocalText.T("Browse Individual Tiles");

        }

        LibraryStatus.Text = (assets == null ? LocalText.T("Libraries not loaded") : LocalText.Format("{0} {1} ready",assets.Libraries.Count,LocalText.T(assets.Libraries.Count == 1 ? "library" : "libraries")));

        LibraryDot.Fill = new ImmutableSolidColorBrush(assets == null ? Color.FromRgb(209, 173, 119) : Color.FromRgb(134, 189, 165));

        if (categoryGrouping != null) UpdateCategoryWorkspace();
        AdjustEmptyLayout();

        SelectedPreview.Source = ActivePreview();

    }

    private void ZoomInClick(object s, RoutedEventArgs e) => Viewport.SetZoom(Viewport.Zoom * 1.2);

    private void ZoomOutClick(object s, RoutedEventArgs e) => Viewport.SetZoom(Viewport.Zoom / 1.2);

    private void ResetZoomClick(object s, RoutedEventArgs e) => Viewport.SetZoom(1);

    private void ClearSearchClick(object s, RoutedEventArgs e) { AssetSearch.Clear(); searchDelay.Stop(); page = 0; BuildPage(); AssetSearch.Focus(); }

    private void AssetAreaSizeChanged(object s, SizeChangedEventArgs e) => AdjustEmptyLayout();

    private void AdjustEmptyLayout()

    {

        if (EmptyIcon == null || AssetArea == null || EmptyAction == null) return;

        bool compact = AssetArea.Bounds.Height < 210;

        EmptyIcon.IsVisible = compact ? false : true;

        EmptyDescription.IsVisible = compact ? false : true;

        EmptyTitle.Margin = compact ? new Thickness(0, 2, 0, 6) : new Thickness(0, 14, 0, 6);

        EmptyAction.Margin = new Thickness(0, compact ? 6 : 16, 0, 0);

        AssetEmpty.Margin = new Thickness(compact ? 4 : 16);

    }

    private void EmptyActionClick(object s, RoutedEventArgs e)

    {
        if (CategoryPalette) { if (categoryError != null) _ = LoadCategoriesAsync(); else ClearSearchClick(s, e); return; }
        if (SavedPalette) { ClearSearchClick(s, e); return; }

        if (StampMode) { if (!string.IsNullOrWhiteSpace(AssetSearch.Text)) ClearSearchClick(s, e); else StampClick(s, e); return; }

        if (assets == null) SettingsClick(s, e);

        else if (!string.IsNullOrWhiteSpace(AssetSearch.Text)) ClearSearchClick(s, e);

        else SetTool(EditorTool.Pencil);

    }

    private void ShortcutsClick(object s, RoutedEventArgs e)

    {

        if (shortcutsWindow is { IsVisible: true }) { shortcutsWindow.Activate(); return; }

        var dialog = new EditorWindow { Title = LocalText.T("InputState Shortcuts"), Width = 460, Height = 680, Owner = this, CanResize = false, WindowStartupLocation = WindowStartupLocation.CenterOwner };

        shortcutsWindow = dialog;
        dialog.Closed += (_, _) => { if (ReferenceEquals(shortcutsWindow, dialog)) shortcutsWindow = null; };

        var panel = new StackPanel { Margin = new(28) }; dialog.Content = panel;

        panel.Children.Add(new TextBlock { Text = LocalText.T("Work Faster"), FontSize = 22, FontWeight = FontWeight.SemiBold, Margin = new(0, 0, 0, 18) });

        foreach (var (action, key) in new[] { ("Brush / Pencil / Special Brush", "B / P / T"), ("Wall / Remove Wall Sections", "H / Shift + Drag"), ("Fill Tiles / Smtiles / Collisions", "F"), ("Eraser / Select", "E / M"), ("Stamp / Pickup / Clone", "O / V / K"), ("Move / Clone Picked Stamp", "Click"), ("Delete Picked Stamp (Pickup)", "Delete"), ("Cancel Pickup / Clone", "Escape"), ("Pan Canvas", "WASD / Space + Drag / Middle Drag"), ("Zoom", "Mouse Wheel"), ("Brush Size", "[ / ]"), ("Previous / Next Palette Item", "Left / Right"), ("Grid / Both Collision Overlays", "G / C"), ("New / Open Map", "Ctrl+N / Ctrl+O"), ("Save Map", "Ctrl+S"), ("Undo / Redo", "Ctrl+Z / Ctrl+Shift+Z"), ("Copy Visible Layers / Paste Region", "Ctrl+C / Ctrl+V"), ("Cut Visible Layers", "Ctrl+X"), ("Erase Selection", "Delete") })

        {

            var row = new DockPanel { Margin = new(0, 0, 0, 14) };

            var shortcut = new TextBlock { Text = LocalText.T(key), Foreground = new ImmutableSolidColorBrush(Color.FromRgb(168, 183, 246)) }; DockPanel.SetDock(shortcut, Dock.Right); row.Children.Add(shortcut); row.Children.Add(new TextBlock { Text = LocalText.T(action)}); panel.Children.Add(row);

        }

        var close = new Button { Content = LocalText.T("Done"), IsCancel = true, HorizontalAlignment = HorizontalAlignment.Right, Padding = new(22, 7, 22, 7) }; panel.Children.Add(close); dialog.Show();

    }

}
