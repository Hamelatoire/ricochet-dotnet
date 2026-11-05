using System.ComponentModel;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Input.Platform;
using Avalonia.Media;
using Avalonia.Interactivity;
using Avalonia.Platform.Storage;
using Avalonia.Styling;
using Avalonia.VisualTree;
using AvaloniaEdit.Document;
using AvaloniaEdit.Highlighting;
using Ricochet.App.ViewModels;
using Ricochet.App.Views;

namespace Ricochet.App;

public partial class MainWindow : Window
{
    private readonly Lazy<IHighlightingDefinition?> _jsonHighlighting;
    private ThemeVariant _theme = ThemeVariant.Dark;
    private MainViewModel? _viewModel;
    private bool _syncingUrl;

    // Drag and drop in the Collections tree. A drag starts only after the pointer moves a little, so a click still selects.
    private const double DragStartDistance = 5;
    private TreeNodeViewModel? _pressedNode;
    private PointerPressedEventArgs? _pressedArgs;
    private Point _pressedAt;
    private TreeNodeViewModel? _draggedNode;
    private TreeViewItem? _hintItem;
    private string? _hintClass;

    public MainWindow()
    {
        InitializeComponent();
        _jsonHighlighting = new(CreateJsonHighlighting);
        InitializeTreeDrag();
        UrlEditor.TextArea.TextView.LineTransformers.Add(new VariableColorizer(new SolidColorBrush(Color.Parse("#FF7A50"))));
        UrlEditor.TextChanged += OnUrlEditorTextChanged;
        UrlEditor.AddHandler(InputElement.KeyDownEvent, OnUrlEditorKeyDown, RoutingStrategies.Tunnel, handledEventsToo: true);
        Opened += (_, _) =>
        {
            UrlEditor.Focus();
            ApplyTheme();
        };
        ActualThemeVariantChanged += (_, _) => ApplyTheme();
        ApplyTheme();
    }

    protected override void OnDataContextChanged(EventArgs e)
    {
        base.OnDataContextChanged(e);

        if (_viewModel is not null)
            _viewModel.PropertyChanged -= OnViewModelPropertyChanged;

        _viewModel = DataContext as MainViewModel;

        if (_viewModel is not null)
        {
            _viewModel.PropertyChanged += OnViewModelPropertyChanged;
            SetUrlText(_viewModel.Url);
        }
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

    private async void OnImportClick(object? sender, RoutedEventArgs e)
    {
        if (_viewModel is null)
            return;

        var files = await StorageProvider.OpenFilePickerAsync(new FilePickerOpenOptions
        {
            Title = "Import Postman files",
            AllowMultiple = true,
            FileTypeFilter =
            [
                new FilePickerFileType("Postman exports") { Patterns = ["*.json"] },
                FilePickerFileTypes.All,
            ],
        });

        var paths = files.Select(f => f.TryGetLocalPath()).OfType<string>().ToList();
        if (paths.Count > 0)
            await _viewModel.ImportFilesAsync(paths);
    }

    private async void OnChangeFolderClick(object? sender, RoutedEventArgs e)
    {
        if (_viewModel is null)
            return;

        var folders = await StorageProvider.OpenFolderPickerAsync(new FolderPickerOpenOptions
        {
            Title = "Choose a workspace folder",
            AllowMultiple = false,
        });

        if (folders.Count > 0 && folders[0].TryGetLocalPath() is { } path)
            await _viewModel.OpenWorkspaceFolderAsync(path);
    }

    private async void OnCopyResponseClick(object? sender, RoutedEventArgs e)
    {
        if (_viewModel is not null && Clipboard is { } clipboard)
            await clipboard.SetTextAsync(_viewModel.ResponseBody);
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
    }

    private void OnUrlEditorKeyDown(object? sender, KeyEventArgs e)
    {
        if (e.Key != Key.Enter || _viewModel is null)
            return;

        e.Handled = true;
        _viewModel.SendOrCancelCommand.Execute(null);
    }

    private void OnViewModelPropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
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
