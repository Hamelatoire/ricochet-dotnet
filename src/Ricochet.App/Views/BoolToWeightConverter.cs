using System.Globalization;
using Avalonia.Data.Converters;
using Avalonia.Media;

namespace Ricochet.App.Views;

/// <summary>Makes collection and folder names stand out from the requests inside them.</summary>
public sealed class BoolToWeightConverter : IValueConverter
{
    public static readonly BoolToWeightConverter Instance = new();

    public object Convert(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        value is true ? FontWeight.Medium : FontWeight.Normal;

    public object ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        throw new NotSupportedException();
}
