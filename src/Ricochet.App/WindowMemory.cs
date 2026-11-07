using System.Runtime.InteropServices;
using System.Runtime.Versioning;

using Avalonia;
using Avalonia.Controls;
using Avalonia.Layout;
using Ricochet.Core;

namespace Ricochet.App;

/// <summary>
/// Restores the main window's size, position and maximised state from the last run, and saves them when it closes.
/// The saved values are personal (state.json), never part of the workspace.
/// </summary>
public sealed class WindowMemory
{
    // A point on the title bar, near its left edge: if it is on a screen, the window can be reached and grabbed.
    private const int GrabX = 40;
    private const int GrabY = 16;

    private readonly Window _window;
    private readonly WorkspaceStore _store;

    // The last size and position the window had while it was not maximised, so maximising doesn't lose them.
    private WindowPlacement? _normal;

    private WindowMemory(Window window, WorkspaceStore store)
    {
        _window = window;
        _store = store;
    }

    /// <summary>Restores the saved placement, if it still fits on a screen, and starts remembering changes. Call before the window is shown.</summary>
    public static void Attach(Window window, WorkspaceStore store)
    {
        var memory = new WindowMemory(window, store);
        memory.Restore();

        window.PositionChanged += (_, _) => memory.Remember();
        window.PropertyChanged += (_, e) =>
        {
            if (e.Property == Layoutable.WidthProperty || e.Property == Layoutable.HeightProperty)
                memory.Remember();
        };
        window.Closing += (_, _) => memory.SaveOnClose();
    }

    private void Restore()
    {
        var saved = _store.Window;

        // Nothing saved, or the window was on a monitor that is no longer there: keep the default size and position.
        if (saved is null || !OnAScreen(saved))
            return;

        // Setting the size and position fires Remember before the whole placement is in place, so _normal is set after them.
        _window.WindowStartupLocation = WindowStartupLocation.Manual;
        _window.Width = saved.Width;
        _window.Height = saved.Height;
        _window.Position = new PixelPoint(saved.X, saved.Y);
        _normal = saved with { IsMaximized = false };

        // Maximising before the window is shown loses the restored position, so it is done once the window has opened.
        if (saved.IsMaximized)
            _window.Opened += (_, _) => _window.WindowState = WindowState.Maximized;
    }

    private bool OnAScreen(WindowPlacement placement)
    {
        var grab = new PixelPoint(placement.X + GrabX, placement.Y + GrabY);
        return _window.Screens.All.Any(screen => screen.WorkingArea.Contains(grab));
    }

    private void Remember()
    {
        if (_window.WindowState == WindowState.Normal)
            _normal = Current();
    }

    private void SaveOnClose()
    {
        var placement = _window.WindowState == WindowState.Normal ? Current() : MaximisedRestoreBounds();
        if (placement is null)
            return;

        _store.SaveWindow(placement with { IsMaximized = _window.WindowState == WindowState.Maximized });
    }

    /// <summary>
    /// The bounds the window returns to when restored. Avalonia can report the maximised position at this point, because
    /// Windows moves a window before Avalonia sees its state change, so on Windows the position comes from Windows itself.
    /// </summary>
    private WindowPlacement? MaximisedRestoreBounds()
    {
        if (_normal is null)
            return null;

        if (OperatingSystem.IsWindows() && NormalPosition() is { } point)
            return _normal with { X = point.X, Y = point.Y };

        return _normal;
    }

    [SupportedOSPlatform("windows")]
    private PixelPoint? NormalPosition()
    {
        var handle = _window.TryGetPlatformHandle()?.Handle ?? IntPtr.Zero;
        if (handle == IntPtr.Zero)
            return null;

        var placement = new NativeMethods.WINDOWPLACEMENT { length = (uint)Marshal.SizeOf<NativeMethods.WINDOWPLACEMENT>() };
        if (!NativeMethods.GetWindowPlacement(handle, ref placement))
            return null;

        return new PixelPoint(placement.rcNormalPosition.Left, placement.rcNormalPosition.Top);
    }

    private WindowPlacement Current() => new()
    {
        X = _window.Position.X,
        Y = _window.Position.Y,
        Width = _window.Bounds.Width,
        Height = _window.Bounds.Height,
    };

    private static class NativeMethods
    {
        [StructLayout(LayoutKind.Sequential)]
        public struct POINT
        {
            public int X;
            public int Y;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct RECT
        {
            public int Left;
            public int Top;
            public int Right;
            public int Bottom;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct WINDOWPLACEMENT
        {
            public uint length;
            public uint flags;
            public uint showCmd;
            public POINT ptMinPosition;
            public POINT ptMaxPosition;
            public RECT rcNormalPosition;
        }

        // Blittable arguments only, so this works under Native AOT without generated marshalling code.
        [SupportedOSPlatform("windows")]
        [DllImport("user32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool GetWindowPlacement(IntPtr hWnd, ref WINDOWPLACEMENT lpwndpl);
    }
}
