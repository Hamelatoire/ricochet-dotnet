using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Ricochet.Core;

namespace Ricochet.App.ViewModels;

/// <summary>
/// The command palette (ADR 0040): one filtered list of actions and Requests, opened with Ctrl+K, or Ctrl+P for Requests
/// only. The shortcuts that move the focus only ask the view to move it, since the view owns focus (ADR 0039).
/// </summary>
public sealed partial class MainViewModel
{
    /// <summary>Raised when the view should move the keyboard focus.</summary>
    public event Action<PaletteFocus>? FocusRequested;

    private List<PaletteEntryViewModel> _paletteEntries = [];

    public ObservableCollection<PaletteEntryViewModel> PaletteResults { get; } = [];

    [ObservableProperty] private bool _isPaletteOpen;

    /// <summary>True when the palette was opened with Ctrl+P, so it lists Requests only.</summary>
    [ObservableProperty] private bool _isPaletteRequestsOnly;

    [ObservableProperty] private PaletteEntryViewModel? _paletteSelected;

    [ObservableProperty] private string _paletteQuery = "";

    partial void OnPaletteQueryChanged(string value) => ApplyPaletteFilter();

    [RelayCommand]
    private void OpenPalette() => OpenPaletteWith(requestsOnly: false);

    [RelayCommand]
    private void FindRequest() => OpenPaletteWith(requestsOnly: true);

    private void OpenPaletteWith(bool requestsOnly)
    {
        var entries = new List<PaletteEntryViewModel>();
        if (!requestsOnly)
            entries.AddRange(ActionEntries());
        entries.AddRange(RequestEntries());
        _paletteEntries = entries;
        IsPaletteRequestsOnly = requestsOnly;
        PaletteQuery = "";
        ApplyPaletteFilter();
        IsPaletteOpen = true;
        FocusRequested?.Invoke(PaletteFocus.PaletteQuery);
    }

    /// <summary>Escape: closes the palette and returns the focus to the URL, where most work starts.</summary>
    [RelayCommand]
    private void ClosePalette()
    {
        IsPaletteOpen = false;
        FocusRequested?.Invoke(PaletteFocus.Url);
    }

    /// <summary>Enter: closes the palette, then runs the highlighted entry.</summary>
    [RelayCommand]
    private void RunPaletteEntry()
    {
        if (PaletteSelected is not { } entry)
            return;

        IsPaletteOpen = false;
        entry.Run();
    }

    [RelayCommand]
    private void MovePaletteUp() => MovePaletteSelection(-1);

    [RelayCommand]
    private void MovePaletteDown() => MovePaletteSelection(1);

    [RelayCommand] private void FocusUrl() => FocusRequested?.Invoke(PaletteFocus.Url);

    [RelayCommand] private void FocusEnvironment() => FocusRequested?.Invoke(PaletteFocus.Environment);

    private void MovePaletteSelection(int step)
    {
        if (PaletteResults.Count == 0)
            return;

        var index = PaletteSelected is null ? -1 : PaletteResults.IndexOf(PaletteSelected);
        PaletteSelected = PaletteResults[Math.Clamp(index + step, 0, PaletteResults.Count - 1)];
    }

    private void ApplyPaletteFilter()
    {
        PaletteResults.Clear();
        foreach (var entry in CommandPalette.Filter(_paletteEntries, PaletteQuery.Trim(), e => e.Name))
            PaletteResults.Add(entry);

        PaletteSelected = PaletteResults.FirstOrDefault();
    }

    private IEnumerable<PaletteEntryViewModel> ActionEntries()
    {
        yield return Entry("New request", Shortcut("N"), () => NewRequestCommand.Execute(null));
        yield return Entry("Send request", Shortcut("Enter"), () => SendOrCancelCommand.Execute(null));
        yield return Entry("Save request", Shortcut("S"), () => SaveCommand.Execute(null));
        yield return Entry("Focus the URL", Shortcut("L"), () => FocusRequested?.Invoke(PaletteFocus.Url));
        yield return Entry("Switch environment", Shortcut("E"), () => FocusRequested?.Invoke(PaletteFocus.Environment));
        yield return Entry("Find a request", Shortcut("P"), () => FindRequestCommand.Execute(null));
        yield return Entry("Bulk run", "", () => OpenBulkPageCommand.Execute(null));
        yield return Entry("Environments", "", () => OpenEnvironmentsPageCommand.Execute(null));
        yield return Entry("Settings", "", () => OpenSettingsPageCommand.Execute(null));
        yield return Entry("Mock server", "", () => Mock.OpenCommand.Execute(null));
        yield return Entry("Import Postman files", "", () => ImportPostmanFilesCommand.Execute(null));
        yield return Entry("Use no environment", "", () => ActiveEnvironment = _noEnvironment);

        foreach (var environment in Environments)
            yield return Entry($"Use environment {environment.Name}", "", () => ActiveEnvironment = environment);
    }

    private IEnumerable<PaletteEntryViewModel> RequestEntries() =>
        AllNodes(Collections)
            .Where(node => node.IsRequest)
            .Select(node => new PaletteEntryViewModel(node.ScopeLabel, node.MethodLabel, () =>
            {
                SelectedNode = node;
                FocusRequested?.Invoke(PaletteFocus.Url);
            }));

    private static PaletteEntryViewModel Entry(string name, string shortcut, Action run) =>
        new(name, shortcut, run);

    /// <summary>The shortcut as it is written on this system: Cmd on macOS, Ctrl elsewhere (ADR 0040).</summary>
    private static string Shortcut(string key) => OperatingSystem.IsMacOS() ? $"Cmd+{key}" : $"Ctrl+{key}";
}
