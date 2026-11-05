using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Ricochet.Core;

namespace Ricochet.App.ViewModels;

public sealed record AssertionOption(string Value, string Label);

public sealed record TestResultRow(string Verdict, string Text, bool Passed);

public sealed partial class AssertionRowViewModel : ObservableObject
{
    private readonly Action<AssertionRowViewModel> _remove;

    public AssertionRowViewModel(RequestAssertion assertion, Action<AssertionRowViewModel> remove)
    {
        _remove = remove;
        _enabled = assertion.Enabled;
        _source = Sources.FirstOrDefault(o => o.Value == assertion.Source) ?? new AssertionOption(assertion.Source, assertion.Source);
        _op = Ops.FirstOrDefault(o => o.Value == assertion.Op) ?? new AssertionOption(assertion.Op, assertion.Op);
        _path = assertion.Path;
        _expected = assertion.Expected;
    }

    public static IReadOnlyList<AssertionOption> Sources { get; } =
    [
        new(AssertionSources.Status, "Status"),
        new(AssertionSources.Time, "Time (ms)"),
        new(AssertionSources.Header, "Header"),
        new(AssertionSources.Body, "Body"),
        new(AssertionSources.Json, "JSON value"),
    ];

    public static IReadOnlyList<AssertionOption> Ops { get; } =
    [
        new(AssertionOps.Equal, "equals"),
        new(AssertionOps.NotEqual, "does not equal"),
        new(AssertionOps.Contains, "contains"),
        new(AssertionOps.NotContains, "does not contain"),
        new(AssertionOps.Exists, "exists"),
        new(AssertionOps.NotExists, "does not exist"),
        new(AssertionOps.LessThan, "is less than"),
        new(AssertionOps.GreaterThan, "is greater than"),
        new(AssertionOps.Matches, "matches pattern"),
    ];

    [ObservableProperty] private bool _enabled;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(NeedsPath), nameof(PathPlaceholder))]
    private AssertionOption _source;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(NeedsExpected))]
    private AssertionOption _op;

    [ObservableProperty] private string _path;
    [ObservableProperty] private string _expected;

    public bool NeedsPath => Source?.Value is AssertionSources.Header or AssertionSources.Json;
    public bool NeedsExpected => Op?.Value is not (AssertionOps.Exists or AssertionOps.NotExists);
    public string PathPlaceholder => Source?.Value == AssertionSources.Header ? "Header name" : "items[0].id";

    [RelayCommand]
    private void Remove() => _remove(this);

    public RequestAssertion ToAssertion() => new()
    {
        Enabled = Enabled,
        Source = Source?.Value ?? AssertionSources.Status,
        Path = NeedsPath ? Path : "",
        Op = Op?.Value ?? AssertionOps.Equal,
        Expected = NeedsExpected ? Expected : "",
    };
}
