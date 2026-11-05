using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Ricochet.Core;

namespace Ricochet.App.ViewModels;

public sealed partial class KeyValueRowViewModel(Action<KeyValueRowViewModel> remove, bool allowSecrets) : ObservableObject
{
    public bool AllowSecrets { get; } = allowSecrets;

    [ObservableProperty] private bool _enabled = true;
    [ObservableProperty] private string _key = "";
    [ObservableProperty] private string _value = "";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ValueMask), nameof(ValuePlaceholder))]
    private bool _isSecret;

    public char ValueMask => IsSecret ? '•' : '\0';

    // A Secret arriving through git or an import has no value here, and this is where the user learns that.
    public string ValuePlaceholder => IsSecret ? "No value on this machine" : "Value";

    [RelayCommand]
    private void Remove() => remove(this);
}

public sealed partial class KeyValueListViewModel(bool allowSecrets = false) : ObservableObject
{
    public ObservableCollection<KeyValueRowViewModel> Items { get; } = [];

    [RelayCommand]
    private void Add() => Items.Add(NewRow());

    private KeyValueRowViewModel NewRow() => new(row => Items.Remove(row), allowSecrets);

    public void Load(IEnumerable<KeyValue> entries)
    {
        Items.Clear();
        foreach (var entry in entries)
        {
            var row = NewRow();
            row.Enabled = entry.Enabled;
            row.Key = entry.Key;
            row.Value = entry.Value;
            row.IsSecret = allowSecrets && entry.IsSecret;
            Items.Add(row);
        }
    }

    public List<KeyValue> ToEntries() =>
        Items.Where(row => row.Key.Trim().Length > 0)
            .Select(row => new KeyValue(row.Key, row.Value, row.Enabled, row.IsSecret))
            .ToList();
}
