using Avalonia;
using Avalonia.Media;
using Avalonia.Styling;

namespace Ricochet.App.Views;

/// <summary>Reads colours defined per theme in App.axaml, for colours that code sets rather than XAML.</summary>
public static class ThemeColors
{
    /// <summary>Returns the colour for a theme. A missing key shows as magenta, so it is easy to spot.</summary>
    public static Color Get(string key, ThemeVariant theme)
    {
        var resources = Application.Current?.Resources;
        if (resources is not null && resources.TryGetResource(key, theme, out var value) && value is ISolidColorBrush brush)
            return brush.Color;

        return Colors.Magenta;
    }
}
