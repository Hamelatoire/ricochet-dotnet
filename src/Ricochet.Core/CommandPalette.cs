namespace Ricochet.Core;

/// <summary>Filters the command palette's entries (ADR 0040) as the user types.</summary>
public static class CommandPalette
{
    /// <summary>
    /// Keeps the entries whose name contains every word of the query, ignoring case. Entries whose name starts with the
    /// first word come first, and otherwise the original order is kept. An empty query keeps everything.
    /// </summary>
    public static IReadOnlyList<T> Filter<T>(IEnumerable<T> entries, string query, Func<T, string> name)
    {
        var all = entries.ToList();
        var words = query.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries);
        if (words.Length == 0)
            return all;

        return all
            .Where(entry => words.All(word => name(entry).Contains(word, StringComparison.OrdinalIgnoreCase)))
            .OrderBy(entry => name(entry).StartsWith(words[0], StringComparison.OrdinalIgnoreCase) ? 0 : 1)
            .ToList();
    }
}
