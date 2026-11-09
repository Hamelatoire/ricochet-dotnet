using Avalonia;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Layout;
using Avalonia.VisualTree;
using Ricochet.App.ViewModels;
using Ricochet.Core;

namespace Ricochet.App.Views;

/// <summary>
/// Shows a key-value table whose key and value columns can be resized by dragging the gap between them, or with the
/// arrow keys on the focused gap (ADR 0074). The split is a share of the two columns, applied to every row.
/// </summary>
public partial class KeyValueEditor : UserControl
{
    // How far an arrow key moves the boundary, as a share of the two columns.
    private const double KeyStep = 0.02;

    private Point? _dragStart;
    private double _dragStartShare;

    public KeyValueEditor()
    {
        InitializeComponent();
        LayoutUpdated += (_, _) => ApplyShare();
        DataContextChanged += (_, _) => ApplyShare();
        AddHandler(PointerPressedEvent, OnPressed, RoutingStrategies.Tunnel | RoutingStrategies.Bubble, handledEventsToo: false);
        AddHandler(PointerMovedEvent, OnMoved, RoutingStrategies.Tunnel | RoutingStrategies.Bubble, handledEventsToo: false);
        AddHandler(PointerReleasedEvent, OnReleased, RoutingStrategies.Tunnel | RoutingStrategies.Bubble, handledEventsToo: false);
        AddHandler(KeyDownEvent, OnKeyDown, RoutingStrategies.Tunnel | RoutingStrategies.Bubble, handledEventsToo: false);
    }

    private KeyValueListViewModel? Table => DataContext as KeyValueListViewModel;

    /// <summary>Sets the key and value column widths of every row to the table's share. Setting an equal width changes nothing, so this is cheap to repeat.</summary>
    private void ApplyShare()
    {
        if (Table is not { } table)
            return;

        var share = table.KeyShare;
        foreach (var row in this.GetVisualDescendants().OfType<Grid>().Where(g => g.Tag as string == "kv-row"))
        {
            if (row.ColumnDefinitions.Count < 4)
                continue;

            var key = row.ColumnDefinitions[1];
            var value = row.ColumnDefinitions[3];
            if (key.Width.Value != share || value.Width.Value != 1 - share)
            {
                key.Width = new GridLength(share, GridUnitType.Star);
                value.Width = new GridLength(1 - share, GridUnitType.Star);
            }
        }
    }

    private void OnPressed(object? sender, PointerPressedEventArgs e)
    {
        if (Table is null || !IsHandle(e.Source) || !e.GetCurrentPoint(this).Properties.IsLeftButtonPressed)
            return;

        _dragStart = e.GetPosition(this);
        _dragStartShare = Table.KeyShare;
        e.Pointer.Capture((IInputElement)e.Source!);
        e.Handled = true;
    }

    private void OnMoved(object? sender, PointerEventArgs e)
    {
        if (Table is null || _dragStart is null || e.Source is not Border handle)
            return;

        var row = handle.GetVisualAncestors().OfType<Grid>().FirstOrDefault(g => g.Tag as string == "kv-row");
        if (row is null || RowSpan(row) is not { } span)
            return;

        // The drag moves the boundary: its share is where the pointer is between the key's left edge and the value's right edge.
        var pointer = e.GetPosition(this).X;
        var shareAtPointer = (pointer - span.Left) / span.Width;
        var clamped = KeyValueColumns.Clamp(shareAtPointer, span.Width);
        Table.KeyShare = clamped;
        e.Handled = true;
    }

    private void OnReleased(object? sender, PointerReleasedEventArgs e)
    {
        if (_dragStart is null)
            return;

        _dragStart = null;
        e.Pointer.Capture(null);
        Table?.EndResize();
        e.Handled = true;
    }

    private void OnKeyDown(object? sender, KeyEventArgs e)
    {
        if (Table is null || e.Source is not Border handle || !IsHandle(handle))
            return;

        var step = e.Key switch
        {
            Key.Left => -KeyStep,
            Key.Right => KeyStep,
            _ => 0,
        };
        if (step == 0)
            return;

        var row = handle.GetVisualAncestors().OfType<Grid>().FirstOrDefault(g => g.Tag as string == "kv-row");
        var width = row is null ? 0 : RowSpan(row)?.Width ?? 0;
        Table.KeyShare = KeyValueColumns.Clamp(Table.KeyShare + step, width);
        Table.EndResize();
        e.Handled = true;
    }

    private static bool IsHandle(object? source) => source is Border { Tag: "kv-handle" };

    /// <summary>The horizontal span from the key column's left edge to the value column's right edge, in this control's coordinates.</summary>
    private Rect? RowSpan(Grid row)
    {
        var key = row.Children.OfType<TextBox>().FirstOrDefault(t => Grid.GetColumn(t) == 1);
        var value = row.Children.OfType<TextBox>().FirstOrDefault(t => Grid.GetColumn(t) == 3);
        if (key is null || value is null)
            return null;

        var left = key.TranslatePoint(new Point(0, 0), this);
        var right = value.TranslatePoint(new Point(value.Bounds.Width, 0), this);
        if (left is null || right is null)
            return null;

        return new Rect(left.Value, right.Value);
    }
}
