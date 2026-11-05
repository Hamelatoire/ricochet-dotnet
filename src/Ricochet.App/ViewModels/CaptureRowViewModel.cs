using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Ricochet.Core;

namespace Ricochet.App.ViewModels;

public sealed partial class CaptureRowViewModel : ObservableObject
{
    private readonly Action<CaptureRowViewModel> _remove;

    public CaptureRowViewModel(RequestCapture capture, Action<CaptureRowViewModel> remove)
    {
        _remove = remove;
        _enabled = capture.Enabled;
        _source = Sources.FirstOrDefault(o => o.Value == capture.Source) ?? new AssertionOption(capture.Source, capture.Source);
        _path = capture.Path;
        _variable = capture.Variable;
    }

    public static IReadOnlyList<AssertionOption> Sources { get; } =
    [
        new(AssertionSources.Json, "JSON value"),
        new(AssertionSources.Header, "Header"),
        new(AssertionSources.Body, "Body"),
    ];

    [ObservableProperty] private bool _enabled;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(NeedsPath), nameof(PathPlaceholder))]
    private AssertionOption _source;

    [ObservableProperty] private string _path;
    [ObservableProperty] private string _variable;

    public bool NeedsPath => Source?.Value is AssertionSources.Header or AssertionSources.Json;
    public string PathPlaceholder => Source?.Value == AssertionSources.Header ? "Header name" : "data.token";

    [RelayCommand]
    private void Remove() => _remove(this);

    public RequestCapture ToCapture() => new()
    {
        Enabled = Enabled,
        Source = Source?.Value ?? AssertionSources.Json,
        Path = NeedsPath ? Path : "",
        Variable = Variable,
    };
}
