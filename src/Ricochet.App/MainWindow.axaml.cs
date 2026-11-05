using System.ComponentModel;
using Avalonia.Controls;
using Avalonia.Input.Platform;
using Avalonia.Interactivity;
using Avalonia.Platform.Storage;
using Avalonia.Styling;
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

    public MainWindow()
    {
        InitializeComponent();
        _jsonHighlighting = new(CreateJsonHighlighting);
        Opened += (_, _) =>
        {
            UrlBox.Focus();
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
            _viewModel.PropertyChanged += OnViewModelPropertyChanged;
    }

    protected override void OnClosing(WindowClosingEventArgs e)
    {
        base.OnClosing(e);
        _viewModel?.SaveBeforeExit();
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
    private void OnViewModelPropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
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
