using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Ricochet.Core;

namespace Ricochet.App.ViewModels;

public sealed partial class KeyValueRowViewModel(
    Action<KeyValueRowViewModel> remove, bool allowSecrets, string keyPlaceholder, string valuePlaceholder) : ObservableObject
{
    public bool AllowSecrets { get; } = allowSecrets;

    public string KeyPlaceholder { get; } = keyPlaceholder;

    [ObservableProperty] private bool _enabled = true;
    [ObservableProperty] private string _key = "";
    [ObservableProperty] private string _value = "";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ValueMask), nameof(ValuePlaceholder))]
    private bool _isSecret;

    /// <summary>Asks for the value each time a Request uses it (ADR 0019).</summary>
    [ObservableProperty] private bool _isPrompt;

    public char ValueMask => IsSecret ? '•' : '\0';

    // A Secret arriving through git or an import has no value here, and this is where the user learns that.
    public string ValuePlaceholder => IsSecret ? "No value on this machine" : valuePlaceholder;

    [RelayCommand]
    private void Remove() => remove(this);
}

public sealed partial class KeyValueListViewModel(
    bool allowSecrets = false, string keyPlaceholder = "Variable name", string valuePlaceholder = "Value") : ObservableObject
{
    public ObservableCollection<KeyValueRowViewModel> Items { get; } = [];

    /// <summary>The key column's share of the key and value columns (ADR 0074). The view applies it to every row.</summary>
    [ObservableProperty] private double _keyShare = KeyValueColumns.DefaultKeyShare;

    /// <summary>Saves a dragged split when the drag ends, so the width is remembered for this table kind.</summary>
    public Action<double>? RememberKeyShare { get; set; }

    public void EndResize() => RememberKeyShare?.Invoke(KeyShare);

    [RelayCommand]
    private void Add() => Items.Add(NewRow());

    private KeyValueRowViewModel NewRow() =>
        new(row => Items.Remove(row), allowSecrets, keyPlaceholder, valuePlaceholder);

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
            row.IsPrompt = allowSecrets && entry.IsPrompt;
            Items.Add(row);
        }
    }

    /// <summary>Sets the value for a key, adding its row if needed. An empty value removes the row, so the key falls back to its default.</summary>
    public void Set(string key, string value, bool secret, bool prompt = false)
    {
        var row = Items.FirstOrDefault(r => r.Key == key);
        if (value.Length == 0)
        {
            if (row is not null)
                Items.Remove(row);
            return;
        }

        if (row is null)
        {
            row = NewRow();
            row.Key = key;
            row.IsSecret = allowSecrets && secret;
            row.IsPrompt = allowSecrets && prompt;
            Items.Add(row);
        }

        row.Value = value;
    }

    /// <summary>Adds a row for a key the list doesn't have yet, keeping its value. A row that exists is left as it is.</summary>
    public void AddIfMissing(string key, string value, bool secret, bool prompt = false)
    {
        if (Items.Any(r => r.Key == key))
            return;

        var row = NewRow();
        row.Key = key;
        row.Value = value;
        row.IsSecret = allowSecrets && secret;
        row.IsPrompt = allowSecrets && prompt;
        Items.Add(row);
    }

    public List<KeyValue> ToEntries() =>
        Items.Where(row => row.Key.Trim().Length > 0)
            .Select(row => new KeyValue(row.Key, row.Value, row.Enabled, row.IsSecret, row.IsPrompt))
            .ToList();
}
