using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Ricochet.Core;

namespace Ricochet.App.ViewModels;

/// <summary>
/// A Request open as a tab (ADR 0041). It points at the Request's node in the Collection, so a rename shows here, and it keeps
/// the edits not yet saved while another tab is showing. Nothing is written to the Workspace by opening or switching a tab.
/// </summary>
public sealed partial class RequestTabViewModel(TreeNodeViewModel node, Action<RequestTabViewModel> activate, Action<RequestTabViewModel> close) : ObservableObject
{
    [ObservableProperty] private TreeNodeViewModel _node = node;

    [ObservableProperty] private bool _isActive;

    /// <summary>The unsent edits, kept while the tab is not showing. Null when the Request is as saved.</summary>
    public ApiRequest? Draft { get; set; }

    /// <summary>True while there are edits that are not saved, so the tab shows a mark.</summary>
    [ObservableProperty] private bool _isDirty;

    public string Id => Node.Id;

    [RelayCommand] private void Activate() => activate(this);

    [RelayCommand] private void Close() => close(this);
}
