using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Ricochet.Core;

namespace Ricochet.App.ViewModels;

/// <summary>A saved example on a Request. Its body is shown only when asked for, as examples can be long.</summary>
public sealed partial class ResponseExampleRowViewModel : ObservableObject
{
    private readonly Action<ResponseExampleRowViewModel> _remove;
    private readonly Action<ResponseExample> _show;

    public ResponseExampleRowViewModel(ResponseExample example, Action<ResponseExampleRowViewModel> remove, Action<ResponseExample> show)
    {
        Example = example;
        _remove = remove;
        _show = show;
    }

    public ResponseExample Example { get; }

    public string Name => Example.Name;
    public string StatusText => $"{Example.StatusCode} {Example.ReasonPhrase}".Trim();
    public bool IsSuccess => Example.StatusCode is >= 200 and < 400;

    [RelayCommand]
    private void Show() => _show(Example);

    [RelayCommand]
    private void Remove() => _remove(this);
}
