using System.Collections.Specialized;
using System.ComponentModel;
using System.IO;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Input.Platform;
using Avalonia.Threading;
using Avalonia.Media;
using Avalonia.Interactivity;
using Avalonia.Platform.Storage;
using Avalonia.Styling;
using Avalonia.VisualTree;
using AvaloniaEdit.Document;
using AvaloniaEdit.Highlighting;
using AvaloniaEdit.Search;
using Ricochet.App.Services;
using Ricochet.App.ViewModels;
using Ricochet.Core;
using Ricochet.App.Views;

namespace Ricochet.App;

public partial class MainWindow : Window, IDialogs, IClipboardService
{
    private readonly Lazy<IHighlightingDefinition?> _jsonHighlighting;
    private ThemeVariant _theme = ThemeVariant.Dark;
    private MainViewModel? _viewModel;
    private bool _syncingUrl;
    private int _suggestStart = -1; // Where the "{{" being completed starts, or -1 when no list is open.

    // Drag and drop in the Collections tree. A drag starts only after the pointer moves a little, so a click still selects.
    private const double DragStartDistance = 5;
    private TreeNodeViewModel? _pressedNode;
    private PointerPressedEventArgs? _pressedArgs;
    private Point _pressedAt;
    private TreeNodeViewModel? _draggedNode;
    private TreeViewItem? _hintItem;
    private string? _hintClass;

    /// <summary>Where crash logs are written (ADR 0088). Set by the app at startup; the Settings page shows and opens it.</summary>
    public string CrashLogDirectory
    {
        get => _crashLogDirectory;
        set
        {
            _crashLogDirectory = value;
            CrashLogPathText.Text = value;
        }
    }

    private string _crashLogDirectory = "";

    /// <summary>Shows only the settings that match the search text, and hides a section once none of its settings match (ADR 0069).</summary>
    private void SettingsSearch_TextChanged(object? sender, TextChangedEventArgs e)
    {
        var matches = SettingsCatalog.Search(SettingsSearchBox.Text).Select(item => item.Key).ToHashSet();
        var rows = this.GetVisualDescendants().OfType<Control>()
            .Where(c => c.Tag is string tag && tag.StartsWith("setting:", StringComparison.Ordinal))
            .ToList();
        foreach (var row in rows)
        {
            row.IsVisible = matches.Contains(((string)row.Tag!)["setting:".Length..]);
        }

        foreach (var section in this.GetVisualDescendants().OfType<Control>()
                     .Where(c => c.Tag is string tag && tag.StartsWith("section:", StringComparison.Ordinal)))
        {
            section.IsVisible = section.GetVisualDescendants().OfType<Control>()
                .Any(c => c.Tag is string tag && tag.StartsWith("setting:", StringComparison.Ordinal) && c.IsVisible);
        }
    }

    /// <summary>Scrolls the Settings page to the section whose button was clicked.</summary>
    private void SettingsSection_Click(object? sender, RoutedEventArgs e)
    {
        if (sender is not Control { Tag: string key })
        {
            return;
        }

        this.GetVisualDescendants().OfType<Control>()
            .FirstOrDefault(c => c.Tag is string tag && tag == "section:" + key)
            ?.BringIntoView();
    }

    /// <summary>Opens the crash log folder in the file manager. The folder is created first, so it opens even before any crash.</summary>
    private async void OpenCrashLogFolder_Click(object? sender, RoutedEventArgs e)
    {
        if (string.IsNullOrEmpty(_crashLogDirectory))
            return;

        Directory.CreateDirectory(_crashLogDirectory);
        var launcher = TopLevel.GetTopLevel(this)?.Launcher;
        if (launcher is not null)
            await launcher.LaunchDirectoryInfoAsync(new DirectoryInfo(_crashLogDirectory));
    }

    public MainWindow()
    {
        InitializeComponent();
        _jsonHighlighting = new(CreateJsonHighlighting);
        // Ctrl+F opens a find bar on the response body (ADR 0045). It searches the text as shown, case-insensitively by default.
        var responseSearch = SearchPanel.Install(ResponseEditor);
        // Escape closes the bar wherever focus is inside it, as it closes the palette and dialogs.
        ResponseEditor.AddHandler(InputElement.KeyDownEvent, (_, e) =>
        {
            if (e.Key == Key.Escape && responseSearch.IsOpened)
            {
                responseSearch.Close();
                e.Handled = true;
            }
        }, RoutingStrategies.Bubble, handledEventsToo: true);
        InitializeTreeDrag();
        InitializeTreeKeys();
        UrlEditor.TextArea.TextView.LineTransformers.Add(new VariableColorizer(ChipsIn));
        UrlEditor.TextChanged += OnUrlEditorTextChanged;
        UrlEditor.PointerMoved += OnUrlEditorPointerMoved;
        UrlEditor.LostFocus += (_, _) => HideSuggestions();
        UrlEditor.AddHandler(InputElement.KeyDownEvent, OnUrlEditorKeyDown, RoutingStrategies.Tunnel, handledEventsToo: true);
        SuggestPopup.PlacementTarget = UrlEditor;
        // The branch can change outside the app, so it's read again whenever the window comes to the front.
        Activated += (_, _) =>
        {
            _viewModel?.RefreshRepositoryStatusCommand.Execute(null);
            UrlEditor.TextArea.TextView.Redraw(); // Variables may have changed on another page, so chips are drawn again.
        };
        Opened += (_, _) =>
        {
            // The Bulk run page must never be showing when the window first appears, so the request is always visible.
            _viewModel?.CloseBulkPageCommand.Execute(null);
            UrlEditor.Focus();
            ApplyTheme();
        };
        // Typing and focus changes may have changed the open Request, so its tab's unsaved mark is checked again (ADR 0041).
        AddHandler(InputElement.KeyUpEvent, (_, _) => _viewModel?.RefreshTabDirtyState(), RoutingStrategies.Bubble, handledEventsToo: true);
        AddHandler(InputElement.PointerReleasedEvent, (_, _) => _viewModel?.RefreshTabDirtyState(), RoutingStrategies.Bubble, handledEventsToo: true);
        AddHandler(InputElement.LostFocusEvent, (_, _) => _viewModel?.RefreshTabDirtyState(), RoutingStrategies.Bubble, handledEventsToo: true);
        ActualThemeVariantChanged += (_, _) => ApplyTheme();
        ApplyTheme();
    }

    protected override void OnDataContextChanged(EventArgs e)
    {
        base.OnDataContextChanged(e);

        if (_viewModel is not null)
        {
            _viewModel.PropertyChanged -= OnViewModelPropertyChanged;
            _viewModel.BulkLog.CollectionChanged -= OnBulkLogChanged;
            _viewModel.FocusRequested -= OnFocusRequested;
        }

        _viewModel = DataContext as MainViewModel;

        if (_viewModel is not null)
        {
            _viewModel.PropertyChanged += OnViewModelPropertyChanged;
            _viewModel.BulkLog.CollectionChanged += OnBulkLogChanged;
            _viewModel.FocusRequested += OnFocusRequested;
            SetUrlText(_viewModel.Url);
        }
    }

    /// <summary>
    /// Moves the keyboard focus when the view model asks. It is posted, so a control that has just been shown can take focus.
    /// </summary>
    private void OnFocusRequested(PaletteFocus target)
    {
        Dispatcher.UIThread.Post(() =>
        {
            switch (target)
            {
                case PaletteFocus.PaletteQuery:
                    PaletteQueryBox.Focus();
                    PaletteQueryBox.CaretIndex = PaletteQueryBox.Text?.Length ?? 0;
                    break;
                case PaletteFocus.Url:
                    UrlEditor.Focus();
                    break;
                case PaletteFocus.Environment:
                    EnvironmentPicker.Focus();
                    break;
            }
        }, DispatcherPriority.Input);
    }

    /// <summary>Running a palette entry on a double click, as Enter does from the search box.</summary>
    private void OnPaletteDoubleTapped(object? sender, TappedEventArgs e) =>
        _viewModel?.RunPaletteEntryCommand.Execute(null);

    /// <summary>While a bulk run is going, keeps the newest log line in view. Scrolling back up stays possible between runs.</summary>
    private void OnBulkLogChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        if (_viewModel is { IsBulkRunning: true } && BulkLogList.ItemCount > 0)
            BulkLogList.ScrollIntoView(BulkLogList.ItemCount - 1);
    }

    protected override void OnClosing(WindowClosingEventArgs e)
    {
        base.OnClosing(e);
        _viewModel?.SaveBeforeExit();
    }

    private void InitializeTreeDrag()
    {
        CollectionsTree.AddHandler(InputElement.PointerPressedEvent, OnTreePointerPressed, RoutingStrategies.Tunnel, handledEventsToo: true);
        CollectionsTree.AddHandler(InputElement.PointerMovedEvent, OnTreePointerMoved, RoutingStrategies.Tunnel, handledEventsToo: true);
        CollectionsTree.AddHandler(InputElement.PointerReleasedEvent, OnTreePointerReleased, RoutingStrategies.Tunnel, handledEventsToo: true);
        DragDrop.SetAllowDrop(CollectionsTree, true);
        DragDrop.AddDragOverHandler(CollectionsTree, OnTreeDragOver);
        DragDrop.AddDragLeaveHandler(CollectionsTree, (_, _) => ClearDropHint());
        DragDrop.AddDropHandler(CollectionsTree, OnTreeDrop);
    }

    // Keys for the Collections tree. Handled before the tree's own keys, so arrows move the selection here, not the focus.
    private void InitializeTreeKeys() =>
        CollectionsTree.AddHandler(InputElement.KeyDownEvent, OnTreeKeyDown, RoutingStrategies.Tunnel);

    private void OnTreeKeyDown(object? sender, KeyEventArgs e)
    {
        // The Menu key and Shift+F10 open the focused item's context menu (ADR 0056).
        if (e.Key == Key.Apps || (e.Key == Key.F10 && e.KeyModifiers == KeyModifiers.Shift))
        {
            e.Handled = OpenItemMenu(e.Source);
            return;
        }

        // The rename box and the row buttons keep their own keys, and Ctrl/Shift/Alt combinations are left for shortcuts.
        if (_viewModel is null || e.KeyModifiers != KeyModifiers.None || e.Source is TextBox or Button)
            return;

        e.Handled = true;
        switch (e.Key)
        {
            case Key.Up:
                _viewModel.MoveTreeSelectionCommand.Execute(-1);
                break;
            case Key.Down:
                _viewModel.MoveTreeSelectionCommand.Execute(1);
                break;
            case Key.Right:
                _viewModel.ExpandTreeSelectionCommand.Execute(null);
                break;
            case Key.Left:
                _viewModel.CollapseTreeSelectionCommand.Execute(null);
                break;
            case Key.Enter:
                _viewModel.OpenTreeSelectionCommand.Execute(null);
                break;
            case Key.F2:
                _viewModel.RenameTreeSelectionCommand.Execute(null);
                break;
            default:
                e.Handled = false;
                break;
        }
    }

    /// <summary>Opens the context menu of the tree row that holds <paramref name="source"/>. Returns false when there is no row.</summary>
    private static bool OpenItemMenu(object? source)
    {
        if (source is not Visual visual || visual.FindAncestorOfType<TreeViewItem>() is not { } item)
            return false;

        if (item.GetVisualDescendants().OfType<Grid>().FirstOrDefault(g => g.Classes.Contains("treerow")) is not { ContextMenu: { } menu } row)
            return false;

        menu.Open(row);
        return true;
    }

    private void OnTreePointerPressed(object? sender, PointerPressedEventArgs e)
    {
        _pressedNode = null;

        // Buttons and the rename box handle their own clicks.
        if (e.Source is Button or TextBox || !e.GetCurrentPoint(CollectionsTree).Properties.IsLeftButtonPressed)
            return;

        if (RowNode(e.Source) is not { IsRenaming: false } node)
            return;

        _pressedNode = node;
        _pressedArgs = e;
        _pressedAt = e.GetPosition(CollectionsTree);
    }

    private void OnTreePointerReleased(object? sender, PointerReleasedEventArgs e) => _pressedNode = null;

    private async void OnTreePointerMoved(object? sender, PointerEventArgs e)
    {
        if (_pressedNode is null || _pressedArgs is null)
            return;

        var moved = e.GetPosition(CollectionsTree) - _pressedAt;
        if (Math.Abs(moved.X) < DragStartDistance && Math.Abs(moved.Y) < DragStartDistance)
            return;

        var node = _pressedNode;
        var args = _pressedArgs;
        _pressedNode = null;
        _pressedArgs = null;

        _draggedNode = node;
        var data = new DataTransfer();
        data.Add(DataTransferItem.CreateText(node.Id));
        try
        {
            await DragDrop.DoDragDropAsync(args, data, DragDropEffects.Move);
        }
        finally
        {
            _draggedNode = null;
            ClearDropHint();
        }
    }

    private void OnTreeDragOver(object? sender, DragEventArgs e)
    {
        if (ResolveDrop(e) is not { } drop)
        {
            e.DragEffects = DragDropEffects.None;
            ClearDropHint();
            return;
        }

        e.DragEffects = DragDropEffects.Move;
        e.Handled = true;
        ShowDropHint(drop.Item, drop.Placement);
    }

    private async void OnTreeDrop(object? sender, DragEventArgs e)
    {
        var dragged = _draggedNode;
        var drop = ResolveDrop(e);
        ClearDropHint();
        if (dragged is null || drop is not { } target || _viewModel is null)
            return;

        e.Handled = true;
        await _viewModel.MoveNodeAsync(dragged, target.Node, target.Placement);
    }

    /// <summary>Where the dragged node would land under the pointer, or null where a drop is not allowed.</summary>
    private (TreeViewItem Item, TreeNodeViewModel Node, DropPlacement Placement)? ResolveDrop(DragEventArgs e)
    {
        if (_draggedNode is null || (e.Source as Visual)?.FindAncestorOfType<TreeViewItem>() is not { } item)
            return null;
        if (RowOf(e.Source) is not { } row || RowNode(e.Source) is not { } target || row.Bounds.Height <= 0)
            return null;

        var fraction = e.GetPosition(row).Y / row.Bounds.Height;
        return _draggedNode.PlacementOnto(target, fraction) is { } placement ? (item, target, placement) : null;
    }

    /// <summary>The row under the pointer: the nearest row grid above it, so a folder's child rows never count as its own row.</summary>
    private static Grid? RowOf(object? source) =>
        (source as Visual)?.GetSelfAndVisualAncestors().OfType<Grid>().FirstOrDefault(g => g.Classes.Contains("treerow"));

    /// <summary>The node of the row under the pointer, if the pointer is on a row of a tree item and not on a child item.</summary>
    private static TreeNodeViewModel? RowNode(object? source) =>
        RowOf(source) is { DataContext: TreeNodeViewModel node } && (source as Visual)?.FindAncestorOfType<TreeViewItem>()?.DataContext == node
            ? node
            : null;

    private void ShowDropHint(TreeViewItem item, DropPlacement placement)
    {
        var cssClass = placement switch
        {
            DropPlacement.Before => "drop-before",
            DropPlacement.After => "drop-after",
            _ => "drop-into",
        };
        if (item == _hintItem && cssClass == _hintClass)
            return;

        ClearDropHint();
        item.Classes.Add(cssClass);
        _hintItem = item;
        _hintClass = cssClass;
    }

    private void ClearDropHint()
    {
        if (_hintItem is not null && _hintClass is not null)
            _hintItem.Classes.Remove(_hintClass);
        _hintItem = null;
        _hintClass = null;
    }

    // The pickers are the platform's own dialogs, so the window owns them and the view model asks for paths (ADR 0039).
    async Task<IReadOnlyList<string>> IDialogs.PickPostmanFilesAsync()
    {
        var files = await StorageProvider.OpenFilePickerAsync(new FilePickerOpenOptions
        {
            Title = "Import Postman files",
            AllowMultiple = true,
            FileTypeFilter =
            [
                new FilePickerFileType("Postman exports") { Patterns = ["*.json", "*.zip"] },
                FilePickerFileTypes.All,
            ],
        });

        return files.Select(f => f.TryGetLocalPath()).OfType<string>().ToList();
    }

    async Task<string?> IDialogs.PickOpenApiFileAsync()
    {
        var files = await StorageProvider.OpenFilePickerAsync(new FilePickerOpenOptions
        {
            Title = "Choose an OpenAPI description",
            AllowMultiple = false,
            FileTypeFilter =
            [
                new FilePickerFileType("OpenAPI in JSON") { Patterns = ["*.json"] },
                FilePickerFileTypes.All,
            ],
        });

        return files.FirstOrDefault()?.TryGetLocalPath();
    }

    async Task<string?> IDialogs.PickIdsFileAsync()
    {
        var files = await StorageProvider.OpenFilePickerAsync(new FilePickerOpenOptions
        {
            Title = "Import ids from a text file",
            AllowMultiple = false,
            FileTypeFilter =
            [
                new FilePickerFileType("Text files") { Patterns = ["*.txt"] },
                FilePickerFileTypes.All,
            ],
        });

        return files.Count == 0 ? null : files[0].TryGetLocalPath();
    }

    async Task<string?> IDialogs.PickWorkspaceFolderAsync()
    {
        var folders = await StorageProvider.OpenFolderPickerAsync(new FolderPickerOpenOptions
        {
            Title = "Choose a workspace folder",
            AllowMultiple = false,
        });

        return folders.Count == 0 ? null : folders[0].TryGetLocalPath();
    }

    async Task<string?> IDialogs.PickClientCertificateAsync()
    {
        var files = await StorageProvider.OpenFilePickerAsync(new FilePickerOpenOptions
        {
            Title = "Choose a client certificate",
            AllowMultiple = false,
            FileTypeFilter = [new FilePickerFileType("Certificate files") { Patterns = ["*.pfx", "*.p12"] }],
        });

        return files.Count == 0 ? null : files[0].TryGetLocalPath();
    }

    async Task<string?> IDialogs.PickResponseSavePathAsync(string suggestedFileName)
    {
        var extension = Path.GetExtension(suggestedFileName);
        var file = await StorageProvider.SaveFilePickerAsync(new FilePickerSaveOptions
        {
            Title = "Save response body",
            SuggestedFileName = suggestedFileName,
            DefaultExtension = extension.TrimStart('.'),
            FileTypeChoices =
            [
                new FilePickerFileType($"{extension.TrimStart('.').ToUpperInvariant()} files") { Patterns = [$"*{extension}"] },
                FilePickerFileTypes.All,
            ],
        });

        return file?.TryGetLocalPath();
    }

    async Task IClipboardService.SetTextAsync(string text)
    {
        if (Clipboard is { } clipboard)
            await clipboard.SetTextAsync(text);
    }

    // AvaloniaEdit's text is not a bindable property, so the response is pushed in from here.
    /// <summary>Keeps the URL editor and the view model's Url in step, without each one echoing the other.</summary>
    private void SetUrlText(string text)
    {
        if (UrlEditor.Text == text)
            return;

        _syncingUrl = true;
        try
        {
            UrlEditor.Document.Text = text;
            UrlEditor.CaretOffset = text.Length;
        }
        finally
        {
            _syncingUrl = false;
        }
    }

    private void OnUrlEditorTextChanged(object? sender, EventArgs e)
    {
        if (_syncingUrl || _viewModel is null)
            return;

        _viewModel.Url = UrlEditor.Text;
        UpdateSuggestions();
    }

    /// <summary>Chips in one line of the URL, for the colorizer. Empty until the view model exists.</summary>
    private IReadOnlyList<Chip> ChipsIn(string line) =>
        PlaceholderChips.Find(line, _viewModel?.VariablesInScope() ?? []);

    /// <summary>Shows what the chip under the pointer resolves to. A Secret's value is never shown.</summary>
    private void OnUrlEditorPointerMoved(object? sender, PointerEventArgs e)
    {
        var tip = _viewModel is null ? null : ChipTipAt(e.GetPosition(UrlEditor.TextArea.TextView));
        if (ToolTip.GetTip(UrlEditor) as string != tip)
            ToolTip.SetTip(UrlEditor, tip);
    }

    private string? ChipTipAt(Point point)
    {
        var position = UrlEditor.TextArea.TextView.GetPosition(point);
        if (position is null)
            return null;

        var offset = UrlEditor.Document.GetOffset(position.Value.Location);
        var variables = _viewModel!.VariablesInScope();
        var chips = PlaceholderChips.Find(UrlEditor.Text, variables);
        // The edges count too, so the tip also shows when the pointer is on a chip's first or last brace.
        return chips.FirstOrDefault(chip => chip.Start <= offset && offset <= chip.End) is { } hit
            ? PlaceholderChips.Tip(hit, variables)
            : null;
    }

    private void OnUrlEditorKeyDown(object? sender, KeyEventArgs e)
    {
        if (_viewModel is null)
            return;

        if (SuggestPopup.IsOpen && HandleSuggestionKey(e.Key))
        {
            e.Handled = true;
            return;
        }

        if (e.KeyModifiers == KeyModifiers.None && HandleChipKey(e.Key))
        {
            e.Handled = true;
            return;
        }

        // Moving the caret away from a "{{" means it is no longer being completed.
        if (e.Key is Key.Left or Key.Right or Key.Home or Key.End)
            HideSuggestions();

        if (e.Key != Key.Enter)
            return;

        e.Handled = true;
        _viewModel.SendOrCancelCommand.Execute(null);
    }

    /// <summary>
    /// A chip acts as one character: the caret steps over it, and Backspace or Delete removes it whole. Its text is
    /// still there to edit by moving the caret inside it, which the normal keys do not do here.
    /// </summary>
    private bool HandleChipKey(Key key)
    {
        if (UrlEditor.SelectionLength > 0)
            return false;

        var caret = UrlEditor.CaretOffset;
        var chips = PlaceholderChips.Find(UrlEditor.Text, _viewModel!.VariablesInScope());
        var inside = PlaceholderChips.Covering(chips, caret);

        switch (key)
        {
            case Key.Left when (inside ?? PlaceholderChips.EndingAt(chips, caret)) is { } left:
                UrlEditor.CaretOffset = left.Start;
                return true;
            case Key.Right when (inside ?? PlaceholderChips.StartingAt(chips, caret)) is { } right:
                UrlEditor.CaretOffset = right.End;
                return true;
            case Key.Back when (inside ?? PlaceholderChips.EndingAt(chips, caret)) is { } back:
                RemoveChip(back);
                return true;
            case Key.Delete when (inside ?? PlaceholderChips.StartingAt(chips, caret)) is { } forward:
                RemoveChip(forward);
                return true;
            default:
                return false;
        }
    }

    private void RemoveChip(Chip chip)
    {
        UrlEditor.Document.Remove(chip.Start, chip.Length);
        UrlEditor.CaretOffset = chip.Start;
    }

    /// <summary>Opens the suggestion list when the text before the caret ends in an unfinished "{{name".</summary>
    private void UpdateSuggestions()
    {
        if (_viewModel is null)
            return;

        var typed = System.Text.RegularExpressions.Regex.Match(UrlEditor.Text[..UrlEditor.CaretOffset], @"\{\{([^{}\r\n]*)$");
        var suggestions = typed.Success
            ? PlaceholderChips.Suggest(_viewModel.VariablesInScope(), typed.Groups[1].Value)
            : [];
        if (suggestions.Count == 0)
        {
            HideSuggestions();
            return;
        }

        _suggestStart = typed.Index;
        SuggestList.ItemsSource = suggestions;
        SuggestList.SelectedIndex = 0;
        SuggestPopup.IsOpen = true;
    }

    private void HideSuggestions()
    {
        _suggestStart = -1;
        SuggestPopup.IsOpen = false;
    }

    /// <summary>Up, Down, Enter, Tab and Escape while the list is open. Other keys are left to the editor.</summary>
    private bool HandleSuggestionKey(Key key)
    {
        var count = SuggestList.ItemCount;
        switch (key)
        {
            case Key.Down when count > 0:
                SuggestList.SelectedIndex = (SuggestList.SelectedIndex + 1) % count;
                return true;
            case Key.Up when count > 0:
                SuggestList.SelectedIndex = (SuggestList.SelectedIndex - 1 + count) % count;
                return true;
            case Key.Enter or Key.Tab when SuggestList.SelectedItem is ChipSuggestion suggestion:
                AcceptSuggestion(suggestion);
                return true;
            case Key.Escape:
                HideSuggestions();
                return true;
            default:
                return false;
        }
    }

    /// <summary>Writes the chosen name in place of the unfinished "{{", closing it unless a "}}" already follows.</summary>
    private void AcceptSuggestion(ChipSuggestion suggestion)
    {
        var start = _suggestStart;
        var caret = UrlEditor.CaretOffset;
        HideSuggestions();
        if (start < 0)
            return;

        var closed = UrlEditor.Text[caret..].StartsWith("}}", StringComparison.Ordinal);
        var written = "{{" + suggestion.Name + (closed ? "" : "}}");
        UrlEditor.Document.Replace(start, caret - start, written);
        UrlEditor.CaretOffset = start + written.Length;
    }

    private void OnViewModelPropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        // Chips depend on Variables and Captures, which may have changed, so the URL is drawn again.
        UrlEditor.TextArea.TextView.Redraw();

        if (e.PropertyName == nameof(MainViewModel.Url) && _viewModel is not null)
            SetUrlText(_viewModel.Url);

        if (e.PropertyName == nameof(MainViewModel.ThemeChoice) && _viewModel is not null && Application.Current is { } app)
        {
            app.RequestedThemeVariant = ThemeSettings.Variant(ThemeSettings.StoredValue(_viewModel.ThemeChoice));
            return;
        }

        if (e.PropertyName != nameof(MainViewModel.ResponseBody) || _viewModel is null)
            return;

        ResponseEditor.SyntaxHighlighting = _viewModel.ResponseIsJson ? _jsonHighlighting.Value : null;
        ResponseEditor.Document = new TextDocument(_viewModel.ResponseBody);
    }

    // Colours set from code follow the theme, so they are applied again whenever the theme changes.
    private void ApplyTheme()
    {
        _theme = ActualThemeVariant == ThemeVariant.Light ? ThemeVariant.Light : ThemeVariant.Dark;
        MethodBrushConverter.ApplyTheme(_theme);

        if (_jsonHighlighting.IsValueCreated && _jsonHighlighting.Value is { } definition)
        {
            ApplyJsonColours(definition);
            ResponseEditor.TextArea.TextView.Redraw();
        }
    }

    private IHighlightingDefinition? CreateJsonHighlighting()
    {
        var definition = HighlightingManager.Instance.GetDefinition("Json");
        if (definition is not null)
            ApplyJsonColours(definition);

        return definition;
    }

    // The built-in JSON colours are for light backgrounds, so the app sets its own for each theme.
    private void ApplyJsonColours(IHighlightingDefinition definition)
    {
        foreach (var color in definition.NamedHighlightingColors)
        {
            var key = color.Name switch
            {
                "FieldName" => "JsonProperty",
                "String" => "JsonString",
                "Number" => "JsonNumber",
                "Bool" or "Null" => "JsonKeyword",
                _ => "JsonText",
            };
            color.Foreground = new SimpleHighlightingBrush(ThemeColors.Get(key, _theme));
        }
    }
}
