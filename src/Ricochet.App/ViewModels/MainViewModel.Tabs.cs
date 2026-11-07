using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Ricochet.Core;

namespace Ricochet.App.ViewModels;

/// <summary>
/// Requests open as tabs across the top of the request view (ADR 0041). The editor still shows one Request at a time. A tab
/// keeps its unsent edits as a draft while another tab is showing, and the draft is loaded back when the tab is shown again.
/// </summary>
public sealed partial class MainViewModel
{
    public ObservableCollection<RequestTabViewModel> OpenTabs { get; } = [];

    [ObservableProperty] private RequestTabViewModel? _activeTab;

    /// <summary>The tab whose closing is waiting for an answer, because it has edits that are not saved.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasTabAwaitingClose), nameof(TabCloseMessage))]
    private RequestTabViewModel? _tabAwaitingClose;

    public bool HasOpenTabs => OpenTabs.Count > 0;

    public bool HasTabAwaitingClose => TabAwaitingClose is not null;

    public string OpenTabsLabel => OpenTabs.Count == 1 ? "1 open" : $"{OpenTabs.Count} open";

    public string TabCloseMessage => TabAwaitingClose is null
        ? ""
        : $"\"{TabAwaitingClose.Node.Name}\" has edits that are not saved. Save them, or discard them and close the tab.";

    private void InitializeTabs() =>
        OpenTabs.CollectionChanged += (_, _) =>
        {
            OnPropertyChanged(nameof(HasOpenTabs));
            OnPropertyChanged(nameof(OpenTabsLabel));
        };

    /// <summary>Opens the Requests that were open last time, and shows the one that was showing. Requests since removed are skipped.</summary>
    private void RestoreTabs(WorkspaceStore store)
    {
        var requests = AllNodes(Collections).Where(n => n.IsRequest).ToList();
        foreach (var id in store.OpenTabIds)
        {
            if (requests.FirstOrDefault(n => n.Id == id) is { } node && OpenTabs.All(t => t.Id != id))
                OpenTabs.Add(NewTab(node));
        }

        if ((OpenTabs.FirstOrDefault(t => t.Id == store.ActiveTabId) ?? OpenTabs.FirstOrDefault()) is { } active)
            ActivateTab(active);
    }

    private RequestTabViewModel NewTab(TreeNodeViewModel node) => new(node, ActivateTab, RequestClose);

    /// <summary>Shows a Request in a tab, opening one if it has none. Used when a Request is picked in the tree.</summary>
    private void ShowInTab(TreeNodeViewModel node)
    {
        var tab = OpenTabs.FirstOrDefault(t => t.Id == node.Id);
        if (tab is null)
        {
            tab = NewTab(node);
            OpenTabs.Add(tab);
        }

        ActivateTab(tab);
    }

    /// <summary>Makes a tab the one showing, loading its unsent edits if it has any.</summary>
    private void ActivateTab(RequestTabViewModel tab)
    {
        if (tab == ActiveTab || tab.Node.Request is not { } saved)
            return;

        StashActiveEdits();
        if (!OpenTabs.Contains(tab))
            OpenTabs.Add(tab);

        foreach (var other in OpenTabs)
            other.IsActive = other == tab;
        ActiveTab = tab;

        IsEnvironmentsPageOpen = false;
        IsSettingsPageOpen = false;
        SelectedHistory = null;
        _currentNode = tab.Node;
        CurrentCollection = tab.Node.Root;
        SelectedNode = tab.Node;
        LoadRequest(tab.Draft ?? saved);
        tab.IsDirty = tab.Draft is not null;
        PersistTabs();
    }

    /// <summary>Keeps the unsent edits in the editor with the tab that is showing, so they survive a switch to another tab.</summary>
    private void StashActiveEdits()
    {
        if (ActiveTab is not { } tab || tab.Node.Request is not { } saved)
            return;

        var current = CurrentRequest();
        tab.Draft = WorkspaceSecrets.SameContent(saved, current) ? null : current;
        tab.IsDirty = tab.Draft is not null;
    }

    /// <summary>Called when the window's focus or typing may have changed the request, so the active tab shows its mark.</summary>
    public void RefreshTabDirtyState()
    {
        if (ActiveTab is { Node.Request: { } saved } tab)
            tab.IsDirty = !WorkspaceSecrets.SameContent(saved, CurrentRequest());
    }

    /// <summary>Clears the tab marks when the editor shows a Request that is not in a tab, such as a new one or one from History.</summary>
    private void DeactivateTabs()
    {
        foreach (var tab in OpenTabs)
            tab.IsActive = false;
        ActiveTab = null;
    }

    /// <summary>Closes a tab. A tab with edits that are not saved asks first.</summary>
    private void RequestClose(RequestTabViewModel tab)
    {
        StashActiveEdits();
        if (tab.Draft is not null)
        {
            TabAwaitingClose = tab;
            return;
        }

        RemoveTab(tab);
    }

    [RelayCommand]
    private void CloseActiveTab()
    {
        if (ActiveTab is { } tab)
            RequestClose(tab);
    }

    [RelayCommand]
    private void CancelTabClose() => TabAwaitingClose = null;

    [RelayCommand]
    private void DiscardAndCloseTab()
    {
        if (TabAwaitingClose is not { } tab)
            return;

        TabAwaitingClose = null;
        RemoveTab(tab);
    }

    [RelayCommand]
    private async Task SaveAndCloseTabAsync()
    {
        if (TabAwaitingClose is not { } tab)
            return;

        TabAwaitingClose = null;
        ActivateTab(tab); // The editor must hold the tab's edits for SaveAsync to write them.
        await SaveAsync();
        if (tab.Draft is null)
            RemoveTab(tab);
    }

    private void RemoveTab(RequestTabViewModel tab)
    {
        var index = OpenTabs.IndexOf(tab);
        OpenTabs.Remove(tab);
        tab.IsActive = false;

        if (tab == ActiveTab)
        {
            ActiveTab = null;
            if (OpenTabs.Count > 0)
                ActivateTab(OpenTabs[Math.Min(index, OpenTabs.Count - 1)]);
            else
                NewRequest();
        }

        PersistTabs();
    }

    /// <summary>A Request that was just created by saving opens as the showing tab, without reloading the editor.</summary>
    private void OpenSavedTab(TreeNodeViewModel node)
    {
        var tab = NewTab(node);
        OpenTabs.Add(tab);
        foreach (var other in OpenTabs)
            other.IsActive = other == tab;
        ActiveTab = tab;
    }

    /// <summary>Closes the tabs of a Request, or of a folder, that is being deleted. Their edits go with them, as the delete warns.</summary>
    private void RemoveTabsWithin(TreeNodeViewModel node)
    {
        foreach (var tab in OpenTabs.Where(t => t.Node.IsWithin(node)).ToList())
            RemoveTab(tab);
    }

    /// <summary>After the Workspace is reloaded its Requests are new objects, so each tab is pointed at its Request again.</summary>
    private void RemapTabs()
    {
        var requests = AllNodes(Collections).Where(n => n.IsRequest).ToDictionary(n => n.Id);
        foreach (var tab in OpenTabs.ToList())
        {
            var previous = tab.Node.Request;
            if (!requests.TryGetValue(tab.Id, out var node))
            {
                if (tab.Draft is not null)
                    Notice = JoinNotices(Notice, $"\"{tab.Node.Name}\" was removed outside Ricochet, so its unsaved edits were dropped.");
                OpenTabs.Remove(tab);
                tab.IsActive = false;
                if (tab == ActiveTab)
                    ActiveTab = null;
                continue;
            }

            tab.Node = node;
            if (tab != ActiveTab && tab.Draft is not null && previous is not null && node.Request is { } changed
                && !WorkspaceSecrets.SameContent(previous, changed))
            {
                Notice = JoinNotices(Notice, $"\"{node.Name}\" changed outside Ricochet. Its unsaved edits in another tab will replace that version when saved.");
            }
        }

        PersistTabs();
    }

    private void PersistTabs() => _store.SetOpenTabs(OpenTabs.Select(t => t.Id).ToList(), ActiveTab?.Id);
}
