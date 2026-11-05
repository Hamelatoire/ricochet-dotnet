using System.Globalization;
using Avalonia.Data.Converters;
using Avalonia.Media;

namespace Ricochet.App.Views;

/// <summary>Gives each HTTP method a colour so lists can be scanned quickly.</summary>
public sealed class MethodBrushConverter : IValueConverter
{
    public static readonly MethodBrushConverter Instance = new();

    private static readonly IBrush Get = new SolidColorBrush(Color.Parse("#4ADE80"));
    private static readonly IBrush Post = new SolidColorBrush(Color.Parse("#FBBF24"));
    private static readonly IBrush Put = new SolidColorBrush(Color.Parse("#60A5FA"));
    private static readonly IBrush Patch = new SolidColorBrush(Color.Parse("#C084FC"));
    private static readonly IBrush Delete = new SolidColorBrush(Color.Parse("#F87171"));
    private static readonly IBrush Other = new SolidColorBrush(Color.Parse("#8A909C"));

    public object Convert(object? value, Type targetType, object? parameter, CultureInfo culture) => value switch
    {
        "GET" => Get,
        "POST" => Post,
        "PUT" => Put,
        "PATCH" => Patch,
        "DELETE" => Delete,
        _ => Other,
    };

    public object ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        throw new NotSupportedException();
}
