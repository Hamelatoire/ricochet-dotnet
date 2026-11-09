using Avalonia;
using Avalonia.Controls;
using Ricochet.Core;

namespace Ricochet.App;

/// <summary>
/// Restores the sidebar's width and the request pane's height from the last run, and saves them when a splitter is let go
/// (ADR 0043). They are personal, so they are kept in the personal state and never in the Workspace.
/// </summary>
public static class PanelMemory
{
    // Room kept for the other panels, so a restored size can never hide the request or response entirely.
    private const double MinMainWidth = 480;
    private const double MinSidebarWidth = 200;
    private const double MinRequestHeight = 160;
    private const double MinResponseHeight = 200;
    private const double DefaultWindowHeight = 900;

    /// <summary>Restores the saved sizes, clamped to the window, and starts remembering splitter changes. Call before the window is shown.</summary>
    public static void Attach(MainWindow window, WorkspaceStore store)
    {
        var columns = window.FindControl<Grid>("MainColumns");
        var sidebarSplitter = window.FindControl<GridSplitter>("SidebarSplitter");
        var requestGrid = window.FindControl<Grid>("RequestResponse");
        var requestSplitter = window.FindControl<GridSplitter>("RequestSplitter");
        if (columns is null || sidebarSplitter is null || requestGrid is null || requestSplitter is null)
            return;

        Restore(window, store, columns, requestGrid);

        sidebarSplitter.DragCompleted += (_, _) => Save(store, columns, requestGrid);
        requestSplitter.DragCompleted += (_, _) => Save(store, columns, requestGrid);
    }

    private static void Restore(MainWindow window, WorkspaceStore store, Grid columns, Grid requestGrid)
    {
        var saved = store.Panels;
        if (saved is null)
            return;

        // The window's size is known from its own restored placement, before it is shown.
        var windowWidth = window.Width;
        var windowHeight = double.IsFinite(window.Height) && window.Height > 0 ? window.Height : DefaultWindowHeight;

        if (saved.SidebarWidth is { } width)
        {
            var widest = Math.Max(MinSidebarWidth, windowWidth - MinMainWidth);
            columns.ColumnDefinitions[0].Width = new GridLength(Math.Clamp(width, MinSidebarWidth, widest), GridUnitType.Pixel);
        }

        if (saved.RequestHeight is { } height)
        {
            var tallest = Math.Max(MinRequestHeight, windowHeight - MinResponseHeight - 120);
            requestGrid.RowDefinitions[0].Height = new GridLength(Math.Clamp(height, MinRequestHeight, tallest), GridUnitType.Pixel);
        }
    }

    private static void Save(WorkspaceStore store, Grid columns, Grid requestGrid) =>
        store.SavePanels(new PanelLayout
        {
            SidebarWidth = columns.ColumnDefinitions[0].ActualWidth,
            RequestHeight = requestGrid.RowDefinitions[0].ActualHeight,
        });
}
