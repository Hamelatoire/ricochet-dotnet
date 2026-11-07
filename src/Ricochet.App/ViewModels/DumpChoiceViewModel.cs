using System.Collections.ObjectModel;
using System.ComponentModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Ricochet.Core;

namespace Ricochet.App.ViewModels;

/// <summary>The choice shown after a Postman data dump is read (ADR 0038). Every export starts selected.</summary>
public sealed partial class DumpChoiceViewModel : ObservableObject
{
    private readonly Func<DumpChoiceViewModel, Task> _import;
    private readonly Action _cancel;

    public DumpChoiceViewModel(string dumpName, IReadOnlyList<DumpEntry> listing, Func<DumpChoiceViewModel, Task> import, Action cancel)
    {
        DumpName = dumpName;
        Listing = listing;
        _import = import;
        _cancel = cancel;
        Entries = new ObservableCollection<DumpChoiceItem>(listing.Select(e => new DumpChoiceItem(e)));
        foreach (var item in Entries)
            item.PropertyChanged += OnItemChanged;
    }

    public string DumpName { get; }

    /// <summary>Every export in the dump, already read. The import uses these, so the file is not read twice.</summary>
    public IReadOnlyList<DumpEntry> Listing { get; }

    public ObservableCollection<DumpChoiceItem> Entries { get; }

    public int SelectedCount => Entries.Count(i => i.IsSelected);

    public bool HasSelection => SelectedCount > 0;

    public string Summary => $"{SelectedCount} of {Entries.Count} selected";

    /// <summary>The file names of the ticked exports, which is what <see cref="PostmanImporter.Selected"/> takes.</summary>
    public IReadOnlySet<string> SelectedFileNames => Entries.Where(i => i.IsSelected).Select(i => i.Entry.FileName).ToHashSet();

    [RelayCommand]
    private Task ImportAsync() => _import(this);

    [RelayCommand]
    private void Cancel() => _cancel();

    private void OnItemChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName != nameof(DumpChoiceItem.IsSelected))
            return;
        OnPropertyChanged(nameof(SelectedCount));
        OnPropertyChanged(nameof(HasSelection));
        OnPropertyChanged(nameof(Summary));
    }
}

/// <summary>One export in the choice, with its tick box.</summary>
public sealed partial class DumpChoiceItem(DumpEntry entry) : ObservableObject
{
    public DumpEntry Entry { get; } = entry;

    public string Label => $"{Entry.Name} ({Entry.Kind})";

    [ObservableProperty]
    private bool _isSelected = true;
}
