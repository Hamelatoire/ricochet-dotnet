using CommunityToolkit.Mvvm.ComponentModel;

namespace Ricochet.App.ViewModels;

/// <summary>One prompt Variable the next send needs. Its value is only held while the prompt is open.</summary>
public sealed partial class PromptVariableRowViewModel(string name, string defaultValue) : ObservableObject
{
    public string Name { get; } = name;

    [ObservableProperty] private string _value = defaultValue;
}
