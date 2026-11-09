namespace Ricochet.Core;

/// <summary>
/// The split between the key and value columns of a key-value table (ADR 0074). It is a proportion of the space the two
/// columns share, not pixels, so a width saved in a small window still fits a larger one.
/// </summary>
public static class KeyValueColumns
{
    /// <summary>The key column's share when nothing was dragged: one part of three, the value column two.</summary>
    public const double DefaultKeyShare = 1.0 / 3.0;

    /// <summary>The narrowest either column may be, in device-independent units, so a few characters always show.</summary>
    public const double MinColumnWidth = 80;

    /// <summary>
    /// Keeps a key share inside the range where both columns are at least <see cref="MinColumnWidth"/> wide.
    /// A value that is not a finite number, or a space too small for both minimums, gives the default.
    /// </summary>
    public static double Clamp(double share, double totalWidth)
    {
        if (!double.IsFinite(share) || !double.IsFinite(totalWidth) || totalWidth < 2 * MinColumnWidth)
            return DefaultKeyShare;

        var low = MinColumnWidth / totalWidth;
        var high = 1 - low;
        return Math.Clamp(share, low, high);
    }

    /// <summary>True for a share that a saved file may hold: a finite proportion strictly between zero and one.</summary>
    public static bool IsSaveable(double share) => double.IsFinite(share) && share is > 0 and < 1;
}
