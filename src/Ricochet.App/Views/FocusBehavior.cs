using Avalonia;
using Avalonia.Controls;
using Avalonia.Threading;

namespace Ricochet.App.Views;

/// <summary>Moves keyboard focus into a control when it becomes visible, such as the box for renaming a folder.</summary>
public static class FocusBehavior
{
    public static readonly AttachedProperty<bool> FocusWhenVisibleProperty =
        AvaloniaProperty.RegisterAttached<Control, bool>("FocusWhenVisible", typeof(FocusBehavior));

    static FocusBehavior()
    {
        Visual.IsVisibleProperty.Changed.AddClassHandler<Control>((control, args) =>
        {
            if (args.GetNewValue<bool>() && GetFocusWhenVisible(control))
            {
                Dispatcher.UIThread.Post(() =>
                {
                    // A new folder can be at the bottom of a long tree, so scroll it into view first.
                    control.BringIntoView();
                    control.Focus();
                    if (control is TextBox box)
                        box.SelectAll();
                }, DispatcherPriority.Input);
            }
        });
    }

    public static bool GetFocusWhenVisible(Control control) => control.GetValue(FocusWhenVisibleProperty);

    public static void SetFocusWhenVisible(Control control, bool value) => control.SetValue(FocusWhenVisibleProperty, value);
}
