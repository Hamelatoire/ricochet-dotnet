using CommunityToolkit.Mvvm.Input;

namespace Ricochet.App.ViewModels;

/// <summary>Keyboard use of the Collections tree: moving between rows, expanding and collapsing, opening and renaming.</summary>
public sealed partial class MainViewModel
{
    /// <summary>The rows the tree shows, top to bottom: each collection, then what is inside it while it is expanded.</summary>
    private List<TreeNodeViewModel> VisibleTreeRows()
    {
        var rows = new List<TreeNodeViewModel>();
        foreach (var collection in Collections)
            AddVisibleRows(collection, rows);
        return rows;
    }

    private static void AddVisibleRows(TreeNodeViewModel node, List<TreeNodeViewModel> rows)
    {
        rows.Add(node);
        if (!node.IsExpanded)
            return;

        foreach (var child in node.VisibleChildren)
            AddVisibleRows(child, rows);
    }

    /// <summary>Up (-1) or Down (+1) to the next row. With nothing selected, the first row is selected.</summary>
    [RelayCommand]
    private void MoveTreeSelection(int step)
    {
        var rows = VisibleTreeRows();
        if (rows.Count == 0)
            return;

        // A selected row inside a collapsed folder is not in the list, so start from the nearest row above it.
        var anchor = SelectedNode;
        while (anchor is not null && !rows.Contains(anchor))
            anchor = anchor.Parent;

        var index = anchor is null ? -1 : rows.IndexOf(anchor);
        SelectedNode = index < 0 ? rows[0] : rows[Math.Clamp(index + step, 0, rows.Count - 1)];
    }

    /// <summary>Right: expands a closed folder or collection, then moves onto its first visible item.</summary>
    [RelayCommand]
    private void ExpandTreeSelection()
    {
        if (SelectedNode is not { IsContainer: true } node)
            return;

        if (!node.IsExpanded)
        {
            node.IsExpanded = true;
            return;
        }

        if (node.VisibleChildren.Count > 0)
            SelectedNode = node.VisibleChildren[0];
    }

    /// <summary>Left: collapses an open folder or collection, otherwise moves up to the row that holds the selected item.</summary>
    [RelayCommand]
    private void CollapseTreeSelection()
    {
        if (SelectedNode is not { } node)
            return;

        if (node.IsContainer && node.IsExpanded)
        {
            node.IsExpanded = false;
            return;
        }

        if (node.Parent is not null)
            SelectedNode = node.Parent;
    }

    /// <summary>Enter: opens a request over the Variables and Bulk run pages, or expands and collapses a folder or collection.</summary>
    [RelayCommand]
    private void OpenTreeSelection()
    {
        if (SelectedNode is not { } node)
            return;

        if (node.IsRequest)
        {
            IsVariablesPageOpen = IsBulkPageOpen = false;
            return;
        }

        node.IsExpanded = !node.IsExpanded;
    }

    /// <summary>F2: starts renaming the selected folder or collection. Requests keep their name in the tree, as before.</summary>
    [RelayCommand]
    private void RenameTreeSelection() => SelectedNode?.ToggleRenameCommand.Execute(null);
}
