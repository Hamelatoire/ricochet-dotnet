using System.ComponentModel;
using Avalonia.Controls;
using Avalonia.Input.Platform;
using Avalonia.Interactivity;
using Avalonia.Media;
using Avalonia.Platform.Storage;
using AvaloniaEdit.Document;
using AvaloniaEdit.Highlighting;
using Ricochet.App.ViewModels;

namespace Ricochet.App;

public partial class MainWindow : Window
{
    private static readonly Lazy<IHighlightingDefinition?> JsonHighlighting = new(CreateJsonHighlighting);

    private MainViewModel? _viewModel;

    public MainWindow()
    {
        InitializeComponent();
        Opened += (_, _) => UrlBox.Focus();
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

        ResponseEditor.SyntaxHighlighting = _viewModel.ResponseIsJson ? JsonHighlighting.Value : null;
        ResponseEditor.Document = new TextDocument(_viewModel.ResponseBody);
    }

    private static IHighlightingDefinition? CreateJsonHighlighting()
    {
        var definition = HighlightingManager.Instance.GetDefinition("Json");
        if (definition is null)
            return null;

        // The built-in colours are meant for light backgrounds.
        foreach (var color in definition.NamedHighlightingColors)
        {
            color.Foreground = new SimpleHighlightingBrush(Color.Parse(color.Name switch
            {
                "FieldName" => "#7DD3FC",
                "String" => "#86EFAC",
                "Number" => "#FDBA74",
                "Bool" or "Null" => "#C4B5FD",
                _ => "#D4D4D8",
            }));
        }

        return definition;
    }
}
