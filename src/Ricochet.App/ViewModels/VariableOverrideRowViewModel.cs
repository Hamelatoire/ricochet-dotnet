using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace Ricochet.App.ViewModels;

/// <summary>
/// A collection variable as the selected environment sees it. Its value is the environment's own value,
/// and an empty value falls back to the collection's default.
/// </summary>
public sealed class VariableOverrideRowViewModel : ObservableObject
{
    private readonly string _defaultValue;
    private readonly KeyValueListViewModel _values;

    public VariableOverrideRowViewModel(string key, string defaultValue, bool isSecret, KeyValueListViewModel values,
        string environmentName, Action<VariableOverrideRowViewModel> move)
    {
        Key = key;
        IsSecret = isSecret;
        _defaultValue = defaultValue;
        _values = values;
        MoveLabel = $"Move to {environmentName}";
        MoveCommand = new RelayCommand(() => move(this));
    }

    public string Key { get; }
    public bool IsSecret { get; }
    public char ValueMask => IsSecret ? '•' : '\0';

    /// <summary>Moves the variable out of the collection and into the environment.</summary>
    public IRelayCommand MoveCommand { get; }
    public string MoveLabel { get; }

    // A secret's default is never shown, the same as in the Environments page.
    public string Placeholder => IsSecret ? "Secret" : _defaultValue.Length > 0 ? $"Default: {_defaultValue}" : "No default";

    public string Value
    {
        get => _values.Items.FirstOrDefault(row => row.Key == Key)?.Value ?? "";
        set
        {
            _values.Set(Key, value, IsSecret);
            OnPropertyChanged();
        }
    }
}
