namespace CrystalStudio;

public sealed class SettingsWindow : EditorWindow
{
    public SettingsWindow(string? currentFolder, Func<string, Task> apply, bool firstSetup = false)
        : this(currentFolder, AssetSourceFormat.Default, (folder, _) => apply(folder), firstSetup) { }

    public SettingsWindow(string? currentFolder, AssetSourceFormat currentFormat,
        Func<string, AssetSourceFormat, Task> apply, bool firstSetup = false)
        : this(new LibrarySettings(currentFolder, AssetFormat: currentFormat), (folder, format, _, _) => apply(folder, format), firstSetup) { }

    public SettingsWindow(LibrarySettings current, Func<string, AssetSourceFormat, AiSettings, bool, Task> apply, bool firstSetup = false)
    {
        string? currentFolder = current.LibraryFolder;
        var currentFormat = current.AssetFormat;
        this.Title = (firstSetup ? LocalText.T("Welcome to Crystal Studio") : LocalText.T("Crystal Studio Settings"));
        Width = 660; SizeToContent = SizeToContent.Height; CanResize = false;
        WindowStartupLocation = WindowStartupLocation.CenterOwner;
        var panel = new StackPanel { Margin = new(24) };
        Content = panel;
        panel.Children.Add(new TextBlock { Text = LocalText.T("Language"), Margin = new(0, 0, 0, 5) });
        int savedLanguageIndex = LocalText.PreferredLanguage() == "zh-CN" ? 0 : 1;
        var languages = new ComboBox { Name = "LanguageSelector", ItemsSource = new[] { "简体中文", "English" }, SelectedIndex = savedLanguageIndex };
        panel.Children.Add(languages);
        var restartHint = new TextBlock { Name = "LanguageRestartHint", Text = LocalText.T("Language changes take effect after restarting Crystal Studio."), TextWrapping = TextWrapping.Wrap, Margin = new(0, 5, 0, 14) };
        panel.Children.Add(restartHint);
        panel.Children.Add(new TextBlock { Text = (firstSetup ? LocalText.T("Choose Your Libraries Once") : LocalText.T("Library Location")), FontSize = 21, Margin = new(0, 0, 0, 12) });
        panel.Children.Add(new TextBlock { Text = LocalText.T("Client root folder (Data and Map)"), Margin = new(0, 0, 0, 5) });
        var folderRow = new DockPanel();
        var browse = new Button { Content = LocalText.T("Browse\u2026")};
        DockPanel.SetDock(browse, Dock.Right); folderRow.Children.Add(browse);
        var folder = new TextBox { Name = "ClientRootFolder", Text = string.IsNullOrWhiteSpace(currentFolder) ? "" : DataPaths.GetClientRoot(currentFolder), IsReadOnly = true, VerticalContentAlignment = VerticalAlignment.Center };
        folderRow.Children.Add(folder); panel.Children.Add(folderRow);
        panel.Children.Add(new TextBlock { Text = LocalText.T("Libraries load from Data; maps are listed from Map. Legacy Data selections are converted to the client root."), TextWrapping = TextWrapping.Wrap, Margin = new(0, 5, 0, 5) });
        panel.Children.Add(new TextBlock { Text = LocalText.T("Asset format"), Margin = new(0, 14, 0, 5) });
        var defaultFormat = new RadioButton { Name = "DefaultAssetFormat", GroupName = "AssetSourceFormat",
            Content = LocalText.T("Default (Crystal libraries)"), IsChecked = currentFormat is not (AssetSourceFormat.Sanxiu or AssetSourceFormat.Shanda) };
        var sanxiuFormat = new RadioButton { Name = "SanxiuAssetFormat", GroupName = "AssetSourceFormat",
            Content = LocalText.T("不紧张\\三修格式"), IsChecked = currentFormat == AssetSourceFormat.Sanxiu };
        var shandaFormat = new RadioButton { Name = "ShandaAssetFormat", GroupName = "AssetSourceFormat",
            Content = LocalText.T("Shanda format (.WZL+.WZX/.WIL+.WIX)"), IsChecked = currentFormat == AssetSourceFormat.Shanda };
        panel.Children.Add(defaultFormat); panel.Children.Add(sanxiuFormat); panel.Children.Add(shandaFormat);
        var preserveClassic = new CheckBox { Name = "PreserveClassicMapFormat", Content = LocalText.T("Preserve classic .map format on save"),
            IsChecked = current.PreserveOriginalMapFormat, Margin = new(0, 10, 0, 0) };
        panel.Children.Add(preserveClassic);
        panel.Children.Add(new TextBlock { Text = LocalText.T("Keep Shanda/Wemade client maps readable by domestic editors and game servers. Turn off only when you need Crystal-only features."),
            TextWrapping = TextWrapping.Wrap, FontSize = 11, Opacity = 0.75, Margin = new(20, 3, 0, 14) });
        var ai = new AiSettingsPanel(current.Ai ?? new());
        panel.Children.Add(new Expander { Name = "AiSettings", Header = LocalText.T("AI settings"), Content = ai,
            HorizontalAlignment = HorizontalAlignment.Stretch, Margin = new(0, 14, 0, 14) });
        Closed += (_, _) => ai.CancelTests();
        var buttons = new StackPanel { Orientation = Orientation.Horizontal, HorizontalAlignment = HorizontalAlignment.Right };
        var cancel = new Button { Content = (firstSetup ? LocalText.T("Set Up Later") : LocalText.T("Cancel")), IsCancel = true };
        var save = new Button { Theme = (Style)this.FindResource("PrimaryButton"), Content = (firstSetup ? LocalText.T("Save and Start") : LocalText.T("Save Changes")), IsDefault = true, IsEnabled = !firstSetup || !string.IsNullOrWhiteSpace(currentFolder) };
        buttons.Children.Add(cancel); buttons.Children.Add(save); panel.Children.Add(buttons);
        var error = new TextBlock { Foreground = new ImmutableSolidColorBrush(Color.FromRgb(228, 158, 151)), TextWrapping = TextWrapping.Wrap, Margin = new(3, 8, 3, 0), IsVisible = false }; panel.Children.Add(error);
        bool changingLanguage = false;
        languages.SelectionChanged += (_, _) =>
        {
            if (changingLanguage) return;
            try
            {
                LocalText.SaveLanguage(languages.SelectedIndex == 0 ? "zh-CN" : "en-US");
                savedLanguageIndex = languages.SelectedIndex;
                restartHint.Text = LocalText.T("Language saved. Restart Crystal Studio to apply.");
            }
            catch (Exception ex) when (ex is System.IO.IOException or UnauthorizedAccessException or System.Text.Json.JsonException)
            {
                StudioLog.Error("Settings.Language",ex);
                changingLanguage = true; languages.SelectedIndex = savedLanguageIndex; changingLanguage = false;
                error.Text = LocalText.Format("Could not save language preference: {0}",ex.Message); error.IsVisible = true;
            }
        };
        bool saving = false;
        Closing += (_, e) => { if (saving) e.Cancel = true; };
        browse.Click += (_, _) =>
        {
            var picker = new OpenFolderDialog { Title = LocalText.T("Select a game folder containing Data and Map") };
            if (!string.IsNullOrWhiteSpace(folder.Text) && System.IO.Directory.Exists(folder.Text)) picker.InitialDirectory = folder.Text;
            if (picker.ShowDialog(this) == true) { folder.Text = DataPaths.GetClientRoot(picker.FolderName); save.IsEnabled = true; }
        };
        save.Click += async (_, _) =>
        {
            error.IsVisible = false; saving = true; save.IsEnabled = false; browse.IsEnabled = false; cancel.IsEnabled = false; languages.IsEnabled = false; defaultFormat.IsEnabled = false; sanxiuFormat.IsEnabled = false; shandaFormat.IsEnabled = false; preserveClassic.IsEnabled = false; save.Content = LocalText.T("Checking Libraries\u2026");
            try
            {
                var nextAi = ai.Read(); ai.IsEnabled = false;
                AssetSourceFormat chosen = shandaFormat.IsChecked == true ? AssetSourceFormat.Shanda
                    : sanxiuFormat.IsChecked == true ? AssetSourceFormat.Sanxiu
                    : AssetSourceFormat.Default;
                await apply(folder.Text ?? "", chosen, nextAi, preserveClassic.IsChecked == true); saving = false; DialogResult = true;
            }
            catch (Exception ex)
            {
                StudioLog.Error("Settings.Save",ex,$"Root={folder.Text}");
                error.Text = (ex.Message); error.IsVisible = true;
                saving = false; ai.IsEnabled = true; save.IsEnabled = true; browse.IsEnabled = true; cancel.IsEnabled = true; languages.IsEnabled = true; defaultFormat.IsEnabled = true; sanxiuFormat.IsEnabled = true; shandaFormat.IsEnabled = true; preserveClassic.IsEnabled = true; save.Content = (firstSetup ? LocalText.T("Save and Start") : LocalText.T("Save Changes"));
            }
        };
    }
}


