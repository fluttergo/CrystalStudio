using System.Globalization;
using System.IO;
using System.Text;
using Avalonia.Controls.Templates;
using CrystalStudio.Rendering;

namespace CrystalStudio;

/// <summary>
/// "Composition Models" palette tab. Mirrors the domestic editor's 模型管理 panel: a grid of
/// black-backed composite thumbnails named &lt;set prefix&gt;_&lt;section+numberBase&gt;, grouped by .set
/// category, with black-key removal and explicit SET import. A .set row is a map cell (X,Y),
/// its IMG number is a global Objects*.wzl index, and the whole section is one composite model.
/// The tab owns its list/preview/paging and hands the chosen model to the existing Stamp tool.
/// </summary>
public partial class MainWindow
{
    private const int SetPageSize = 24;
    private const int SetThumbSize = 108;
    private const int SetCacheLimit = 240;
    private const int SetExportSize = 2048;

    private readonly ComboBox setCategoryBox = new() { Name = "SetCategoryBox", MinWidth = 118, FontSize = 11 };
    private readonly TextBox setCompositionSearch = new() { Name = "SetCompositionSearch", Watermark = LocalText.T("Search composition models…"), Height = 28, MinWidth = 130, Padding = new Thickness(6, 3), Margin = new Thickness(4, 0, 0, 0) };
    private readonly CheckBox setRemoveBlack = new() { Name = "SetRemoveBlack", Content = LocalText.T("Remove black background"), FontSize = 11, Margin = new Thickness(8, 0, 0, 0) };
    private readonly CheckBox setZeroBased = new() { Name = "SetZeroBased", Content = LocalText.T("0-based numbering"), FontSize = 11, Margin = new Thickness(8, 0, 0, 0) };
    private readonly CheckBox setExportFrames = new() { Name = "SetExportFrames", Content = LocalText.T("Export light frames"), FontSize = 11, Margin = new Thickness(8, 0, 0, 0), IsChecked = true };
    private readonly Button setImportButton = new() { Name = "SetImportButton", Content = LocalText.T("Import SET"), FontSize = 11, Padding = new Thickness(9, 4), Margin = new Thickness(8, 0, 0, 0) };
    private readonly ListBox setCompositionList = new() { Name = "SetCompositionList", BorderThickness = new Thickness(0), Background = Brushes.Transparent, MinHeight = 160 };
    private readonly TextBlock setCompositionStatus = new() { FontSize = 10, TextWrapping = TextWrapping.Wrap, Foreground = Brushes.Gray };
    private readonly TextBlock setCompositionDetail = new() { FontSize = 10, TextWrapping = TextWrapping.Wrap, Foreground = Brushes.Gray, VerticalAlignment = VerticalAlignment.Center };
    private readonly Button setPreviousButton = new() { Name = "SetPreviousButton", Content = LocalText.T("Previous"), FontSize = 11, Padding = new Thickness(7, 4) };
    private readonly Button setNextButton = new() { Name = "SetNextButton", Content = LocalText.T("Next"), FontSize = 11, Padding = new Thickness(7, 4), Margin = new Thickness(5, 0, 0, 0) };
    private readonly Button setRefreshButton = new() { Name = "SetRefreshButton", Content = LocalText.T("Refresh"), FontSize = 11, Padding = new Thickness(7, 4), Margin = new Thickness(5, 0, 0, 0) };
    private readonly Button setPlaceButton = new() { Name = "SetPlaceButton", Content = LocalText.T("Place as Stamp"), FontSize = 11, Padding = new Thickness(9, 4), Margin = new Thickness(5, 0, 0, 0), IsEnabled = false };
    private readonly Button setExportButton = new() { Name = "SetExportButton", Content = LocalText.T("Export PNG"), FontSize = 11, Padding = new Thickness(7, 4), Margin = new Thickness(5, 0, 0, 0), IsEnabled = false };
    private readonly Button setExportAllButton = new() { Name = "SetExportAllButton", Content = LocalText.T("Export All"), FontSize = 11, Padding = new Thickness(7, 4), Margin = new Thickness(5, 0, 0, 0), IsEnabled = false };

    private readonly Dictionary<string, RasterImage> setThumbCache = new();
    private SetCompositionTable? setTable;
    private ISetImageBinding? setBinding;
    private IReadOnlyList<SetComposition> setFiltered = [];
    private int setPage;
    private bool setLoading;
    private bool setCategoryChanging;
    private bool setRebinding;
    private bool setNumberingApplying;
    private string setCategory = "";
    private CancellationTokenSource? setCancellation;
    private int setPreviewVersion;
    private int setDetailVersion;
    private Control? setCompositionPanel;
    private TabItem? setCompositionTab;

    // Drag-and-drop: a .set card can be dragged onto the open map to compose it, the same
    // way the resource-panel model library drops objects into the current map.
    private const string SetDragFormat = "application/x-crystal-set-composition";
    private Point setDragStart;
    private SetCard? setDragCard;
    private bool setDropHooked;
    private bool setExporting;

    private sealed record SetCard(SetComposition Model, string Name, int Index, string Detail, Avalonia.Media.IImage? Thumbnail);

    /// <summary>Look up a Strings.json entry by key (T/Format themselves look up by English text).</summary>
    private static string SetText(string key) => LocalText.T(LocalText.Source(key));

    private Control CreateSetCompositionPanel()
    {
        var grid = new Grid { Margin = new Thickness(6),
            RowDefinitions = new RowDefinitions("Auto,Auto,*,Auto,Auto") };
        // The thumbnail grid owns the remaining space. Give the star row a floor so the
        // compact palette pane cannot squeeze it to zero height (which previously left
        // only the black preview box visible).
        grid.RowDefinitions[2].MinHeight = 160;

        var toolbar = new WrapPanel { Orientation = Orientation.Horizontal, Margin = new Thickness(0, 0, 0, 4),
            Children = { setCategoryBox, setCompositionSearch, setRemoveBlack, setZeroBased, setExportFrames, setImportButton } };
        grid.Children.Add(toolbar);

        Grid.SetRow(setCompositionStatus, 1); grid.Children.Add(setCompositionStatus);

        setCompositionList.ItemsPanel = new FuncTemplate<Panel?>(() => new WrapPanel { Orientation = Orientation.Horizontal });
        setCompositionList.ItemTemplate = new FuncDataTemplate<SetCard>((card, _) => BuildSetCard(card));
        setCompositionList.SetValue(ScrollViewer.VerticalScrollBarVisibilityProperty, ScrollBarVisibility.Auto);
        setCompositionList.SetValue(ScrollViewer.HorizontalScrollBarVisibilityProperty, ScrollBarVisibility.Disabled);
        Grid.SetRow(setCompositionList, 2); grid.Children.Add(setCompositionList);
        Avalonia.Automation.AutomationProperties.SetName(setCompositionList, LocalText.T("Composition model list"));

        // Domestic editors show only the thumbnail grid; no separate empty preview box,
        // which otherwise renders as a stray black rectangle when nothing is selected.
        setCompositionDetail.Margin = new Thickness(0, 4, 0, 0);
        Grid.SetRow(setCompositionDetail, 3); grid.Children.Add(setCompositionDetail);

        var buttons = new WrapPanel { Orientation = Orientation.Horizontal, Margin = new Thickness(0, 6, 0, 0),
            Children = { setPreviousButton, setNextButton, setRefreshButton, setPlaceButton, setExportButton, setExportAllButton } };
        Grid.SetRow(buttons, 4); grid.Children.Add(buttons);

        setCategoryBox.SelectionChanged += (_, _) =>
        {
            if (setCategoryChanging) return;
            setCategory = setCategoryBox.SelectedIndex <= 0 ? "" : (setCategoryBox.SelectedItem as string ?? "");
            setPage = 0; FilterSetCompositions();
        };
        setRemoveBlack.IsCheckedChanged += (_, _) =>
        {
            lock (setThumbCache) setThumbCache.Clear();
            if (setTable != null) FilterSetCompositions();
        };
        ToolTip.SetTip(setZeroBased, SetText("SetModels_ZeroBasedHint"));
        setZeroBased.IsCheckedChanged += (_, _) =>
        {
            if (setNumberingApplying) return;
            bool zero = setZeroBased.IsChecked == true;
            SetNumbering.Base = zero ? 0 : 1;
            lock (setThumbCache) setThumbCache.Clear();
            if (setTable != null) FilterSetCompositions();
            try
            {
                settings = settingsStore.Load() with { ZeroBasedSetNumbers = zero };
                settingsStore.Save(settings);
            }
            catch (Exception ex)
            { StudioLog.Warning("Set.Numbering", "Cannot persist numbering preference", ex); }
        };
        setCompositionSearch.TextChanged += (_, _) => { setPage = 0; FilterSetCompositions(); };
        setCompositionList.SelectionChanged += (_, _) =>
        {
            if (setRebinding) return;
            if (setCompositionList.SelectedItem is SetCard card) _ = UpdateSetDetailAsync(card);
            else { setPlaceButton.IsEnabled = false; setCompositionDetail.Text = ""; }
        };
        setCompositionList.DoubleTapped += (_, _) =>
        { if (setCompositionList.SelectedItem is SetCard card) PlaceSetComposition(card); };
        setPreviousButton.Click += (_, _) => { if (setPage > 0) { setPage--; FilterSetCompositions(); } };
        setNextButton.Click += (_, _) => { if ((setPage + 1) * SetPageSize < setFiltered.Count) { setPage++; FilterSetCompositions(); } };
        setRefreshButton.Click += async (_, _) => await RefreshSetCompositionsAsync(settings.LibraryFolder);
        setImportButton.Click += async (_, _) => await ImportSetAsync();
        setPlaceButton.Click += (_, _) =>
        { if (setCompositionList.SelectedItem is SetCard card) PlaceSetComposition(card); };
        setExportButton.Click += async (_, _) => await ExportSetPngAsync();
        setExportAllButton.Click += async (_, _) => await ExportSetPngAsync(exportAll: true);
        Closed += (_, _) => { setCancellation?.Cancel(); setCancellation = null; };
        UpdateSetCompositionControls();
        EnableSetCompositionDragDropOnViewport();
        return grid;
    }

    /// <summary>Apply the persisted numbering preference once settings are loaded at startup.</summary>
    internal void ApplySetNumbering(bool zeroBased)
    {
        setNumberingApplying = true;
        try
        {
            SetNumbering.Base = zeroBased ? 0 : 1;
            setZeroBased.IsChecked = zeroBased;
            lock (setThumbCache) setThumbCache.Clear();
            if (setTable != null) FilterSetCompositions();
        }
        finally { setNumberingApplying = false; }
    }

    private Control BuildSetCard(SetCard? card)
    {
        var border = new Border { Width = 118, Height = 140, Margin = new Thickness(3),
            Background = Brushes.Black, BorderBrush = Brushes.DimGray, BorderThickness = new Thickness(1),
            Cursor = new Cursor(StandardCursorType.Hand) };
        var stack = new StackPanel();
        stack.Children.Add(new Image { Width = SetThumbSize, Height = SetThumbSize,
            Stretch = Stretch.Uniform, Margin = new Thickness(4, 4, 4, 0), Source = card?.Thumbnail });
        stack.Children.Add(new TextBlock { Text = card?.Name ?? "", FontSize = 10,
            Foreground = Brushes.WhiteSmoke, Margin = new Thickness(4, 0, 4, 0), TextTrimming = TextTrimming.CharacterEllipsis });
        stack.Children.Add(new TextBlock { Text = card?.Index.ToString() ?? "", FontSize = 9,
            Foreground = Brushes.Gray, Margin = new Thickness(4, 0, 4, 0) });
        border.Child = stack;
        if (card != null) ToolTip.SetTip(border, card.Name + " · " + card.Detail);
        // Make the card a drag source: a small left-button drag starts an Avalonia drag
        // operation carrying the SetCard; the map viewport accepts the drop (see below).
        if (card != null)
        {
            border.PointerPressed += (_, e) =>
            {
                if (e.GetCurrentPoint(border).Properties.IsLeftButtonPressed)
                {
                    setDragStart = e.GetPosition(border);
                    setDragCard = card;
                }
            };
            border.PointerMoved += (_, e) =>
            {
                if (setDragCard == null) return;
                if (!e.GetCurrentPoint(border).Properties.IsLeftButtonPressed) { setDragCard = null; return; }
                Point pos = e.GetPosition(border);
                if (Math.Abs(pos.X - setDragStart.X) > 4 || Math.Abs(pos.Y - setDragStart.Y) > 4)
                {
                    SetCard payload = setDragCard; setDragCard = null;
                    var data = new DataObject();
                    data.Set(SetDragFormat, payload);
                    _ = DragDrop.DoDragDrop(e, data, DragDropEffects.Copy);
                }
            };
            border.PointerReleased += (_, _) => { setDragCard = null; };
            border.PointerCaptureLost += (_, _) => { setDragCard = null; };
        }
        return border;
    }

    /// <summary>
    /// Wire the map viewport as a drop target for .set compositions. Called once; the viewport
    /// already exists when the Composition Models panel is created.
    /// </summary>
    private void EnableSetCompositionDragDropOnViewport()
    {
        if (setDropHooked || Viewport == null) return;
        setDropHooked = true;
        DragDrop.SetAllowDrop(Viewport, true);
        Viewport.AddHandler(DragDrop.DragOverEvent, OnSetDragOver);
        Viewport.AddHandler(DragDrop.DropEvent, OnSetDragDrop);
    }

    private void OnSetDragOver(object? sender, DragEventArgs e)
    {
        if (e.Data.Contains(SetDragFormat) || e.Data.Contains(ExtractionDrag.Candidate))
        {
            e.DragEffects = DragDropEffects.Copy;
            e.Handled = true;
        }
        else e.DragEffects = DragDropEffects.None;
    }

    private void OnSetDragDrop(object? sender, DragEventArgs e)
    {
        if (!e.Data.Contains(SetDragFormat) && !e.Data.Contains(ExtractionDrag.Candidate)) return;
        e.Handled = true;

        // ---- .set composition path ----
        if (e.Data.Contains(SetDragFormat))
        {
            if (setBinding == null || assets == null)
            {
                Status.Text = LocalText.T("Load game libraries in Settings to preview compositions.");
                return;
            }
            if (e.Data.Get(SetDragFormat) is not SetCard card) return;
            var cells = SetCompositionResolver.ToStampCells(card.Model, setBinding);
            if (cells.Count == 0)
            {
                Status.Text = LocalText.T("No resolvable images in this composition.");
                return;
            }
            Point dropPoint = e.GetPosition(Viewport);
            bool ok = Viewport.DropSetComposition(cells, card.Name, dropPoint);
            if (ok)
            {
                // Keep the composition loaded as the active stamp so further clicks place more copies.
                Viewport.Stamp = cells;
                selectedStampName = card.Name;
                SetTool(EditorTool.Stamp);
                Status.Text = LocalText.Format("Placed {0} cells from {1} · click to place more", cells.Count, card.Name);
            }
            return;
        }

        // ---- extracted object path (from the Object Extraction window) ----
        if (e.Data.Get(ExtractionDrag.Candidate) is ExtractionCandidate candidate)
        {
            if (assets == null)
            {
                Status.Text = LocalText.T("Load game libraries in Settings before placing extracted objects.");
                return;
            }
            var cells = ExtractionContent.Normalize(candidate.Cells);
            if (cells.Count == 0)
            {
                Status.Text = LocalText.T("This object has no foreground fragments to place.");
                return;
            }
            // The candidate's cells reference raw game library ids; they must resolve in the
            // current map's asset set, otherwise the drop would reference an unloaded library.
            var missing = cells.Where(c => c.Cell.HasForeground)
                .Select(c => c.Cell.Foreground!.Library)
                .Distinct().Where(id => !assets.Libraries.ContainsKey(id)).ToArray();
            if (missing.Length > 0)
            {
                Status.Text = LocalText.Format("Cannot place: library {0} not loaded. Load the same resource folder in Settings.",
                    string.Join(", ", missing));
                return;
            }
            Point dropPoint = e.GetPosition(Viewport);
            bool ok = Viewport.DropSetComposition(cells, candidate.Name, dropPoint);
            if (ok)
            {
                // Keep the object loaded as the active stamp so further clicks place more copies.
                Viewport.Stamp = cells;
                selectedStampName = candidate.Name;
                SetTool(EditorTool.Stamp);
                Status.Text = LocalText.Format("Placed {0} cells from {1} · click to place more", cells.Count, candidate.Name);
            }
            return;
        }
    }

    private static string? FindSetFolder(string? folder)
    {
        if (string.IsNullOrWhiteSpace(folder)) return null;
        try
        {
            string full = Path.GetFullPath(folder);
            var candidates = new[] { full, Path.Combine(full, "data"), Path.Combine(full, "Data"),
                DataPaths.ResolveLibraryFolder(full) };
            foreach (string candidate in candidates.Distinct(StringComparer.OrdinalIgnoreCase))
                if (Directory.Exists(candidate) && Directory.EnumerateFiles(candidate, "*.set",
                        new EnumerationOptions { MatchCasing = MatchCasing.CaseInsensitive, IgnoreInaccessible = true }).Any())
                    return Path.GetFullPath(candidate);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException or NotSupportedException) { }
        return null;
    }

    private async Task RefreshSetCompositionsAsync(string? selectedFolder)
    {
        setCancellation?.Cancel();
        var request = setCancellation = new CancellationTokenSource();
        var token = request.Token;
        setLoading = true;
        // Do not clear the loaded table here: an older concurrent refresh must never wipe
        // the result of a newer one. The table is replaced atomically after the scan succeeds.
        setCompositionStatus.Text = LocalText.T("Reading composition models…");
        UpdateSetCompositionControls();
        try
        {
            string? folder = FindSetFolder(selectedFolder);
            if (folder == null)
            {
                // Keep the previously loaded table: a refresh triggered by an unrelated
                // resource reload must not blank a model list that is already usable.
                if (ReferenceEquals(setCancellation, request) && setTable == null)
                    setCompositionStatus.Text = LocalText.T("No .set files found. Choose a game folder containing Data and Map.");
                return;
            }
            var (table, binding) = await Task.Run(() =>
            {
                var t = SetCompositionFile.Load(folder, token);
                var b = SetImageBindingFactory.Create(folder, assets);
                return (t, b);
            }, token);
            if (closed || token.IsCancellationRequested || !ReferenceEquals(setCancellation, request)) return;
            setTable = table; setBinding = binding;
            setPage = 0;
            lock (setThumbCache) setThumbCache.Clear();
            setCategoryChanging = true;
            UpdateSetCategories(table);
            setCategoryChanging = false;
            FilterSetCompositions();
            setCompositionStatus.Text = LocalText.Format("Loaded {0:N0} compositions from {1} · {2:N0} pieces · binding: {3}",
                table.Compositions.Count, string.Join(" + ", table.Sources), table.ImagePieces, binding.Description);
            ToolTip.SetTip(setCompositionStatus, string.Join("\n", table.Warnings.Take(20)));
        }
        catch (OperationCanceledException) when (token.IsCancellationRequested) { }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException)
        { setCompositionStatus.Text = LocalText.Format("Could not read .set files: {0}", ex.Message); }
        finally
        {
            if (!closed && ReferenceEquals(setCancellation, request))
            { setCancellation = null; setLoading = false; UpdateSetCompositionControls(); }
            request.Dispose();
        }
    }

    private async Task ImportSetAsync()
    {
        var picker = new OpenFileDialog { Title = LocalText.T("Import SET"), Filter = "Shanda SET (*.set)|*.set" };
        if (picker.ShowDialog(this) != true || string.IsNullOrWhiteSpace(picker.FileName) || setTable == null) return;
        try
        {
            var token = setCancellation?.Token ?? CancellationToken.None;
            var updated = await Task.Run(() => SetCompositionFile.Import(setTable, picker.FileName, token), token);
            if (closed) return;
            setTable = updated;
            lock (setThumbCache) setThumbCache.Clear();
            setCategoryChanging = true; UpdateSetCategories(updated); setCategoryChanging = false;
            FilterSetCompositions();
            Status.Text = LocalText.Format("Imported {0}", Path.GetFileName(picker.FileName));
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException)
        { setCompositionStatus.Text = LocalText.Format("Could not read .set files: {0}", ex.Message); }
    }

    /// <summary>
    /// Export the current composition models (the filtered set, or the single selected one) as
    /// PNG images, one file per composition. Each is rendered at native isometric resolution
    /// (capped at SetExportSize) using the same painter's-algorithm composite as the thumbnails.
    /// When more than one .set category is present the files are grouped into per-category
    /// subfolders so the export is already organized for later categorisation. The "Remove black
    /// background" toggle controls whether the PNG keeps its black backdrop or is made transparent.
    /// </summary>
    /// <summary>
    /// Export the current composition models (the filtered set, or the single selected one) as
    /// PNG images, one file per composition. Alongside the images a machine-readable manifest is
    /// written (<c>composition_manifest.json</c> + <c>composition_manifest.csv</c>) mapping each
    /// exported file to its category and grid extent, so the batch can be categorised / indexed.
    /// </summary>
    private async Task ExportSetPngAsync(bool exportAll = false)
    {
        if (setBinding == null || assets == null)
        {
            Status.Text = SetText("SetModels_ExportNoLibs");
            return;
        }
        var selected = setCompositionList.SelectedItem as SetCard;
        IReadOnlyList<SetComposition> items = exportAll
            ? (setTable?.Compositions ?? [])
            : selected != null ? new List<SetComposition> { selected.Model } : setFiltered;
        if (items.Count == 0)
        {
            Status.Text = SetText("SetModels_ExportZero");
            return;
        }
        var picker = new OpenFolderDialog { Title = SetText("SetModels_ExportPngTitle") };
        if (picker.ShowDialog(this) != true || string.IsNullOrWhiteSpace(picker.FolderName)) return;
        string root = picker.FolderName;
        bool removeBlack = setRemoveBlack.IsChecked == true;
        bool exportFrames = setExportFrames.IsChecked == true;
        bool groupByCategory = items.Select(c => c.Category).Distinct(StringComparer.OrdinalIgnoreCase).Count() > 1;
        setExporting = true; UpdateSetCompositionControls();
        Status.Text = SetText("SetModels_Exporting");
        using var exportCts = new CancellationTokenSource();
        int exported = 0;
        var records = new List<SetExportRecord>();
        try
        {
            var binding = setBinding; var store = assets;
            await Task.Run(() =>
            {
                foreach (var model in items)
                {
                    exportCts.Token.ThrowIfCancellationRequested();
                    if (binding == null || store == null) break;
                    using var image = SetCompositionResolver.Render(model, store, binding, SetExportSize, exportCts.Token, removeBlack);
                    if (image == null) continue;
                    string dir = root;
                    string categoryDir = "";
                    if (groupByCategory) { categoryDir = SanitizeName(model.Category); dir = Path.Combine(root, categoryDir); Directory.CreateDirectory(dir); }
                    string fileName = SanitizeName(model.DisplayName) + ".png";
                    string file = Path.Combine(dir, fileName);
                    try
                    {
                        using var fs = File.Create(file); image.Save(fs); exported++;
                        string relative = categoryDir.Length == 0 ? fileName : Path.Combine(categoryDir, fileName);
                        lock (records) records.Add(SetExportRecord.From(model, relative));
                    }
                    catch (Exception ex) when (ex is IOException or UnauthorizedAccessException) { StudioLog.Warning("Set.Export", $"Cannot write {file}: {ex.Message}"); }

                    // A .set light row is an animated cell (column 5 = frame count). Write the
                    // whole loop as sequential PNGs in a sibling folder, so the effect can be
                    // previewed / classified / turned into a sprite sheet outside the editor.
                    if (!exportFrames) continue;
                    int frameCount = Math.Clamp(model.AnimationFrames, 0, 64);
                    if (frameCount <= 1) continue;
                    string animDir = Path.Combine(dir, SanitizeName(model.DisplayName) + "_frames");
                    try
                    {
                        Directory.CreateDirectory(animDir);
                        for (int f = 0; f < frameCount; f++)
                        {
                            exportCts.Token.ThrowIfCancellationRequested();
                            using var step = SetCompositionResolver.Render(model, store, binding,
                                SetExportSize, exportCts.Token, removeBlack, f);
                            if (step == null) continue;
                            using var stepStream = File.Create(Path.Combine(animDir, $"frame_{f:00}.png"));
                            step.Save(stepStream);
                        }
                    }
                    catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
                    { StudioLog.Warning("Set.ExportFrames", $"Cannot write frames for {model.DisplayName}: {ex.Message}"); }
                }
            }, exportCts.Token);
        }
        catch (OperationCanceledException) { }
        catch (Exception ex) { StudioLog.Error("Set.Export", ex); }
        finally { setExporting = false; UpdateSetCompositionControls(); }

        string manifestNote = "";
        if (records.Count > 0)
        {
            try
            {
                WriteSetExportManifest(root, records, removeBlack);
                manifestNote = SetText("SetModels_ExportManifest");
            }
            catch (Exception ex) { StudioLog.Error("Set.ExportManifest", ex); }
        }
        Status.Text = LocalText.Format(LocalText.Source("SetModels_Exported"), exported, root) + manifestNote;
    }

    /// <summary>One row of the export manifest: links a written PNG to its composition metadata.</summary>
    private sealed record SetExportRecord(
        string FileName, string Category, string DisplayName, int Section,
        int GridWidth, int GridHeight, int ImagePieces, int CellCount, string Source,
        int AnimationFrames, int AnimationTick)
    {
        public string GridRange => $"{GridWidth}×{GridHeight}";
        public static SetExportRecord From(SetComposition model, string relativePath) =>
            new(relativePath, model.Category, model.DisplayName, model.Section,
                model.GridWidth, model.GridHeight, model.ImagePieces, model.CellCount, model.Source,
                model.AnimationFrames, model.AnimationTick);
    }

    /// <summary>
    /// Hand-write the export manifest as JSON + CSV. The JSON is built manually (no reflection
    /// serializer) so it keeps working under the NativeAOT publish, which disables reflection-based
    /// JSON. The CSV carries a UTF-8 BOM so Excel opens the Chinese headers correctly.
    /// </summary>
    private static void WriteSetExportManifest(string root, IReadOnlyList<SetExportRecord> records, bool removeBlack)
    {
        string timestamp = DateTime.Now.ToString("yyyy-MM-dd HH:mm:ss", CultureInfo.InvariantCulture);

        var sb = new StringBuilder();
        sb.Append('{');
        sb.Append("\"generated\":").Append(JsonString(timestamp)).Append(',');
        sb.Append("\"root\":").Append(JsonString(root)).Append(',');
        sb.Append("\"removeBlackBackground\":").Append(removeBlack ? "true" : "false").Append(',');
        sb.Append("\"numberBase\":").Append(SetNumbering.Base.ToString(CultureInfo.InvariantCulture)).Append(',');
        sb.Append("\"count\":").Append(records.Count.ToString(CultureInfo.InvariantCulture)).Append(',');
        sb.Append("\"entries\":[");
        for (int i = 0; i < records.Count; i++)
        {
            var r = records[i];
            if (i > 0) sb.Append(',');
            sb.Append('{');
            sb.Append("\"fileName\":").Append(JsonString(r.FileName)).Append(',');
            sb.Append("\"category\":").Append(JsonString(r.Category)).Append(',');
            sb.Append("\"displayName\":").Append(JsonString(r.DisplayName)).Append(',');
            sb.Append("\"section\":").Append(r.Section.ToString(CultureInfo.InvariantCulture)).Append(',');
            sb.Append("\"section0\":").Append(r.Section.ToString(CultureInfo.InvariantCulture)).Append(',');
            sb.Append("\"section1\":").Append((r.Section + 1).ToString(CultureInfo.InvariantCulture)).Append(',');
            sb.Append("\"gridWidth\":").Append(r.GridWidth.ToString(CultureInfo.InvariantCulture)).Append(',');
            sb.Append("\"gridHeight\":").Append(r.GridHeight.ToString(CultureInfo.InvariantCulture)).Append(',');
            sb.Append("\"gridRange\":").Append(JsonString(r.GridRange)).Append(',');
            sb.Append("\"imagePieces\":").Append(r.ImagePieces.ToString(CultureInfo.InvariantCulture)).Append(',');
            sb.Append("\"cellCount\":").Append(r.CellCount.ToString(CultureInfo.InvariantCulture)).Append(',');
            sb.Append("\"animated\":").Append(r.AnimationFrames > 1 ? "true" : "false").Append(',');
            sb.Append("\"animationFrames\":").Append(r.AnimationFrames.ToString(CultureInfo.InvariantCulture)).Append(',');
            sb.Append("\"animationTick\":").Append(r.AnimationTick.ToString(CultureInfo.InvariantCulture)).Append(',');
            sb.Append("\"source\":").Append(JsonString(r.Source));
            sb.Append('}');
        }
        sb.Append("]}");
        File.WriteAllText(Path.Combine(root, "composition_manifest.json"), sb.ToString(), Encoding.UTF8);

        var csv = new StringBuilder();
        csv.Append('\uFEFF'); // UTF-8 BOM for Excel
        csv.AppendLine("文件名,分类,显示名,区块,格范围,网格宽,网格高,图块数,单元数,来源,编号(0起),编号(1起),编号基准,灯光帧数,帧间隔");
        foreach (var r in records)
            csv.AppendLine(string.Join(",", CsvField(r.FileName), CsvField(r.Category), CsvField(r.DisplayName),
                r.Section.ToString(CultureInfo.InvariantCulture), CsvField(r.GridRange),
                r.GridWidth.ToString(CultureInfo.InvariantCulture), r.GridHeight.ToString(CultureInfo.InvariantCulture),
                r.ImagePieces.ToString(CultureInfo.InvariantCulture), r.CellCount.ToString(CultureInfo.InvariantCulture),
                CsvField(r.Source), r.Section.ToString(CultureInfo.InvariantCulture),
                (r.Section + 1).ToString(CultureInfo.InvariantCulture),
                SetNumbering.Base.ToString(CultureInfo.InvariantCulture),
                r.AnimationFrames.ToString(CultureInfo.InvariantCulture),
                r.AnimationTick.ToString(CultureInfo.InvariantCulture)));
        File.WriteAllText(Path.Combine(root, "composition_manifest.csv"), csv.ToString(), Encoding.UTF8);
    }

    private static string JsonString(string value)
    {
        var sb = new StringBuilder(value.Length + 2);
        sb.Append('"');
        foreach (char c in value)
        {
            switch (c)
            {
                case '"': sb.Append("\\\""); break;
                case '\\': sb.Append("\\\\"); break;
                case '\n': sb.Append("\\n"); break;
                case '\r': sb.Append("\\r"); break;
                case '\t': sb.Append("\\t"); break;
                default:
                    if (c < 0x20) sb.Append("\\u").Append(((int)c).ToString("x4", CultureInfo.InvariantCulture));
                    else sb.Append(c);
                    break;
            }
        }
        sb.Append('"');
        return sb.ToString();
    }

    private static string CsvField(string value)
    {
        if (value.Contains(',') || value.Contains('"') || value.Contains('\n') || value.Contains('\r'))
            return "\"" + value.Replace("\"", "\"\"") + "\"";
        return value;
    }

    private static string SanitizeName(string value)
    {
        string cleaned = new string(value.Where(c => c is not ('/' or '\\' or ':' or '*' or '?' or '"' or '<' or '>' or '|' or '\0')).ToArray()).Trim();
        return cleaned.Length == 0 ? "set" : cleaned;
    }

    private void UpdateSetCategories(SetCompositionTable table)
    {
        string previous = setCategory;
        setCategoryBox.Items.Clear();
        setCategoryBox.Items.Add(LocalText.T("All categories"));
        foreach (string category in table.CategoryList) setCategoryBox.Items.Add(category);
        int index = 0;
        for (int i = 0; i < table.CategoryList.Count; i++)
            if (table.CategoryList[i].Equals(previous, StringComparison.OrdinalIgnoreCase)) { index = i + 1; break; }
        setCategoryBox.SelectedIndex = index;
        setCategory = index == 0 ? "" : table.CategoryList[index - 1];
    }

    private bool MatchesSetFilter(SetComposition composition, string query)
    {
        if (setCategory.Length > 0 && !composition.Source.Split(" + ", StringSplitOptions.RemoveEmptyEntries)
                .Any(s => SetCompositionFile.Prefix(s).Equals(setCategory, StringComparison.OrdinalIgnoreCase)))
            return false;
        return query.Length == 0 ||
            composition.DisplayName.Contains(query, StringComparison.OrdinalIgnoreCase) ||
            composition.Section.ToString().Contains(query, StringComparison.OrdinalIgnoreCase) ||
            composition.Category.Contains(query, StringComparison.OrdinalIgnoreCase);
    }

    private static string ThumbKey(SetCard card, bool removeBlack) =>
        card.Model.DisplayName + "|" + card.Index + "|" + (removeBlack ? "t" : "b");

    private void FilterSetCompositions()
    {
        if (setTable == null) { setCompositionList.ItemsSource = null; setFiltered = []; UpdateSetCompositionControls(); return; }
        string query = setCompositionSearch.Text?.Trim() ?? "";
        setFiltered = setTable.Compositions.Where(c => MatchesSetFilter(c, query)).ToArray();
        int pages = Math.Max(1, (setFiltered.Count + SetPageSize - 1) / SetPageSize);
        setPage = Math.Clamp(setPage, 0, pages - 1);
        bool removeBlack = setRemoveBlack.IsChecked == true;
        var cards = new SetCard[Math.Min(SetPageSize, Math.Max(0, setFiltered.Count - setPage * SetPageSize))];
        for (int i = 0; i < cards.Length; i++)
        {
            var model = setFiltered[setPage * SetPageSize + i];
            // A selected category acts as the naming prefix, matching domestic editors
            // (e.g. choosing mir2 shows mir2_1, mir2_2 ...).
            string name = setCategory.Length > 0 ? SetNumbering.Name(setCategory, model.Section) : model.DisplayName;
            var card = new SetCard(model, name, setPage * SetPageSize + i, model.Detail, null);
            string key = ThumbKey(card, removeBlack);
            lock (setThumbCache) if (setThumbCache.TryGetValue(key, out var cached)) card = card with { Thumbnail = cached };
            cards[i] = card;
        }
        setRebinding = true;
        setCompositionList.ItemsSource = cards;
        setCompositionList.SelectedIndex = -1;
        setRebinding = false;
        setCompositionDetail.Text = setFiltered.Count == 0 ? LocalText.T("No matching composition models.") :
            LocalText.Format("Page {0} of {1}", setPage + 1, pages);
        UpdateSetCompositionControls();
        _ = RenderSetThumbnailsAsync(cards, ++setPreviewVersion);
    }

    private async Task RenderSetThumbnailsAsync(SetCard[] cards, int version)
    {
        var binding = setBinding;
        var store = assets;
        if (binding == null || store == null || cards.Length == 0) return;
        bool removeBlack = setRemoveBlack.IsChecked == true;
        // A refresh owns and disposes its CancellationTokenSource; thumbnails use their
        // own version stamp instead so a disposed token can never abort the whole batch.
        var token = CancellationToken.None;
        var jobs = new List<(int Index, string Key, SetComposition Model)>();
        for (int i = 0; i < cards.Length; i++)
            if (cards[i].Thumbnail == null) jobs.Add((i, ThumbKey(cards[i], removeBlack), cards[i].Model));
        if (jobs.Count == 0) return;
        List<(int Index, string Key, RasterImage? Image)> rendered;
        try
        {
            rendered = await Task.Run(() =>
            {
                var result = new List<(int Index, string Key, RasterImage? Image)>();
                foreach (var job in jobs)
                {
                    if (version != setPreviewVersion) break;
                    RasterImage? image = null;
                    try { image = SetCompositionResolver.Render(job.Model, store, binding, SetThumbSize, token, removeBlack); }
                    catch (Exception ex) when (ex is IOException or ArgumentException or InvalidDataException or OperationCanceledException)
                    { StudioLog.Warning("Set.Thumbnail", $"Model={job.Model.DisplayName}; IMG render failed", ex); }
                    result.Add((job.Index, job.Key, image));
                }
                return result;
            });
        }
        catch (Exception ex) { StudioLog.Error("Set.Thumbnails", ex); return; }
        if (closed || version != setPreviewVersion) return;
        lock (setThumbCache)
        {
            foreach (var r in rendered) if (r.Image != null) setThumbCache[r.Key] = r.Image;
            while (setThumbCache.Count > SetCacheLimit)
            {
                string oldest = setThumbCache.Keys.First();
                setThumbCache.Remove(oldest);
            }
        }
        var updated = cards.ToArray();
        foreach (var r in rendered) if (r.Image != null) updated[r.Index] = updated[r.Index] with { Thumbnail = r.Image };
        int selected = setCompositionList.SelectedIndex;
        setRebinding = true;
        setCompositionList.ItemsSource = updated;
        if (selected >= 0 && selected < updated.Length) setCompositionList.SelectedIndex = selected;
        setRebinding = false;
    }

    private void UpdateSetCompositionControls()
    {
        setPreviousButton.IsEnabled = !setLoading && setPage > 0;
        setNextButton.IsEnabled = !setLoading && (setPage + 1) * SetPageSize < setFiltered.Count;
        setRefreshButton.IsEnabled = !setLoading && !busy;
        setImportButton.IsEnabled = !setLoading && setTable != null;
        setCategoryBox.IsEnabled = setCompositionSearch.IsEnabled = setCompositionList.IsEnabled = !setLoading;
        setPlaceButton.IsEnabled = !setLoading && setCompositionList.SelectedItem is SetCard;
        setExportButton.IsEnabled = !setLoading && !setExporting && setTable != null;
        setExportAllButton.IsEnabled = !setLoading && !setExporting && setTable != null;
    }

    private async Task UpdateSetDetailAsync(SetCard card)
    {
        int version = ++setDetailVersion;
        var binding = setBinding;
        var store = assets;
        setPlaceButton.IsEnabled = binding != null && store != null;
        if (binding == null || store == null)
        { setCompositionDetail.Text = LocalText.T("Load game libraries in Settings to preview compositions."); return; }
        try
        {
            var (stats, placedCells) = await Task.Run(() =>
                (SetCompositionResolver.Measure(card.Model, store, binding),
                 SetCompositionResolver.ToStampCells(card.Model, binding).Count));
            if (closed || version != setDetailVersion) return;
            setCompositionDetail.Text = LocalText.Format("Bound {0:N0}/{1:N0} images · decoded {2:N0} · {3}",
                stats.Bound, stats.Total, stats.Decoded, card.Detail)
                + " · " + LocalText.Format("Placed as {0} cells", placedCells);
        }
        catch (Exception ex) when (ex is IOException or ArgumentException or InvalidDataException)
        { setCompositionDetail.Text = ex.Message; }
    }

    private void PlaceSetComposition(SetCard card)
    {
        if (setBinding == null) return;
        var cells = SetCompositionResolver.ToStampCells(card.Model, setBinding);
        if (cells.Count == 0)
        { Status.Text = LocalText.T("No resolvable images in this composition."); return; }
        Viewport.Stamp = cells;
        selectedStampPath = null;
        selectedStampName = card.Name;
        selectedStampPreview = null;
        SetTool(EditorTool.Stamp);
        Viewport.InvalidateVisual();
        Status.Text = LocalText.Format("Placed {0} cells from {1}", cells.Count, card.Name);
    }
}
