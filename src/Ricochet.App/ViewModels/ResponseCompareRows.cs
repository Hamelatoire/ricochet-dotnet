using Avalonia.Media;
using Ricochet.Core;

namespace Ricochet.App.ViewModels;

/// <summary>One line of the side-by-side comparison. A side without the line shows an empty cell.</summary>
public sealed record CompareLineRow(string PreviousNumber, string Previous, string CurrentNumber, string Current, IBrush PreviousBrush, IBrush CurrentBrush)
{
    // Translucent tints, so the same colours read on both themes.
    private static readonly IBrush Removed = new SolidColorBrush(Color.Parse("#3DE5484D"));
    private static readonly IBrush Added = new SolidColorBrush(Color.Parse("#3D46A758"));
    private static readonly IBrush Changed = new SolidColorBrush(Color.Parse("#3DF5A623"));

    public static CompareLineRow From(SideBySideLine line)
    {
        var previousBrush = line.Kind switch
        {
            DiffKind.Removed or DiffKind.Changed => line.Kind == DiffKind.Removed ? Removed : Changed,
            _ => Brushes.Transparent,
        };
        var currentBrush = line.Kind switch
        {
            DiffKind.Added => Added,
            DiffKind.Changed => Changed,
            _ => Brushes.Transparent,
        };

        return new CompareLineRow(
            line.PreviousNumber?.ToString(System.Globalization.CultureInfo.InvariantCulture) ?? "",
            line.Previous ?? "",
            line.CurrentNumber?.ToString(System.Globalization.CultureInfo.InvariantCulture) ?? "",
            line.Current ?? "",
            previousBrush,
            currentBrush);
    }
}

/// <summary>One header in the comparison. A changed value is tinted, and a hidden one is shown as hidden.</summary>
public sealed record CompareHeaderRow(string Name, string Previous, string Current, IBrush Brush)
{
    private static readonly IBrush Changed = new SolidColorBrush(Color.Parse("#3DF5A623"));

    public static CompareHeaderRow From(HeaderDifference header) => new(
        header.Name,
        header.Previous ?? "—",
        header.Current ?? "—",
        header.Changed ? Changed : Brushes.Transparent);
}
