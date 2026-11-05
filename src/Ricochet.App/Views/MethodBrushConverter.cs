using System.Globalization;
using Avalonia.Data.Converters;
using Avalonia.Media;
using Avalonia.Styling;

namespace Ricochet.App.Views;

/// <summary>Gives each HTTP method a colour so lists can be scanned quickly.</summary>
public sealed class MethodBrushConverter : IValueConverter
{
    public static readonly MethodBrushConverter Instance = new();

    // The brushes stay the same objects, so a theme change recolours rows that are already on screen.
    private static readonly Dictionary<string, (string Resource, SolidColorBrush Brush)> Methods = new()
    {
        ["GET"] = ("MethodGet", new SolidColorBrush()),
        ["POST"] = ("MethodPost", new SolidColorBrush()),
        ["PUT"] = ("MethodPut", new SolidColorBrush()),
        ["PATCH"] = ("MethodPatch", new SolidColorBrush()),
        ["DELETE"] = ("MethodDelete", new SolidColorBrush()),
    };

    private static readonly (string Resource, SolidColorBrush Brush) Other = ("MethodOther", new SolidColorBrush());

    /// <summary>Sets every method colour to the one for the given theme.</summary>
    public static void ApplyTheme(ThemeVariant theme)
    {
        foreach (var (resource, brush) in Methods.Values.Append(Other))
            brush.Color = ThemeColors.Get(resource, theme);
    }

    public object Convert(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        value is string method && Methods.TryGetValue(method, out var entry) ? entry.Brush : Other.Brush;

    public object ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        throw new NotSupportedException();
}
