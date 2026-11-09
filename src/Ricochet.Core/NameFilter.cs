namespace Ricochet.Core;

/// <summary>
/// The name filter on the Collection tree (ADR 0050). A Folder or Request is shown when its own name matches,
/// or when something inside it does, so every match keeps its parents. Matching ignores case.
/// </summary>
public static class NameFilter
{
    /// <summary>The query as typed, trimmed. Empty means the filter is off.</summary>
    public static string Normalize(string? query) => (query ?? "").Trim();

    /// <summary>Whether a name contains the query. An empty query matches everything.</summary>
    public static bool Matches(string name, string query) =>
        Normalize(query).Length == 0 || name.Contains(Normalize(query), StringComparison.OrdinalIgnoreCase);

    /// <summary>
    /// Whether a node is shown: its name matches, or any child is shown. Children are checked lazily, stopping at the first one shown.
    /// Hidden children must be left out of <paramref name="childrenShown"/> by the caller (ADR 0032).
    /// </summary>
    public static bool Shows(string name, string query, IEnumerable<bool> childrenShown) =>
        Matches(name, query) || childrenShown.Any(shown => shown);
}
