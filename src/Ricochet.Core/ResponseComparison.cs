namespace Ricochet.Core;

/// <summary>How a row of a side-by-side body comparison differs. Same rows match on both sides.</summary>
public enum DiffKind
{
    Same,
    Removed,
    Added,
    Changed,
}

/// <summary>
/// One row of a side-by-side body comparison. A side without the line is null, and its line number too.
/// </summary>
public sealed record SideBySideLine(DiffKind Kind, int? PreviousNumber, string? Previous, int? CurrentNumber, string? Current);

/// <summary>
/// A header on either response. A hidden header (a sensitive one, as in History) shows only that it is hidden,
/// and is never counted as changed, so the comparison cannot reveal its value (ADR 0070).
/// </summary>
public sealed record HeaderDifference(string Name, string? Previous, string? Current, bool Hidden)
{
    public bool Changed => !Hidden && !string.Equals(Previous, Current, StringComparison.Ordinal);
}

/// <param name="Body">Rows of the formatted bodies, empty when the body was not compared.</param>
/// <param name="BodyNote">Why the body was not compared, or null when it was.</param>
/// <param name="Headers">Every header name on either response, in order.</param>
public sealed record ResponseComparison(
    IReadOnlyList<SideBySideLine> Body,
    string? BodyNote,
    IReadOnlyList<HeaderDifference> Headers);

/// <summary>
/// Compares two responses for the view that shows them side by side (ADR 0070). Bodies are compared after
/// pretty-printing, so whitespace alone does not make every line differ. Secret values are masked before
/// comparing, so the output never shows them.
/// </summary>
public static class ResponseComparer
{
    // Masking very short values would scramble unrelated output, as in ResponseExamples.
    private const int MinMaskedLength = 4;

    // The line-by-line table is a grid of line pairs, so very large bodies are not compared.
    private const long MaxLinePairs = 4_000_000;

    public static ResponseComparison Compare(ApiResponse previous, ApiResponse current, IEnumerable<string>? secrets = null)
    {
        var masks = (secrets ?? [])
            .Where(s => s.Length >= MinMaskedLength)
            .Distinct()
            .OrderByDescending(s => s.Length)
            .ToList();

        var (body, note) = CompareBodies(previous, current, masks);
        return new ResponseComparison(body, note, CompareHeaders(previous.Headers, current.Headers, masks));
    }

    private static (IReadOnlyList<SideBySideLine> Body, string? Note) CompareBodies(ApiResponse previous, ApiResponse current, List<string> masks)
    {
        var before = ResponseFormatter.Format(previous);
        var after = ResponseFormatter.Format(current);
        if (before.IsBinary || after.IsBinary)
            return ([], "Binary responses are not compared.");

        var a = SplitLines(Mask(before.Text, masks));
        var b = SplitLines(Mask(after.Text, masks));
        if ((long)a.Length * b.Length > MaxLinePairs)
            return ([], "The bodies are too large to compare line by line.");

        return (DiffLines(a, b), null);
    }

    private static string[] SplitLines(string text) => text.Replace("\r\n", "\n", StringComparison.Ordinal).Split('\n');

    /// <summary>Longest common subsequence of lines, then a walk that pairs each run of changes into Changed rows.</summary>
    private static List<SideBySideLine> DiffLines(string[] a, string[] b)
    {
        int n = a.Length, m = b.Length;
        var lcs = new int[(n + 1) * (m + 1)];
        int Index(int i, int j) => i * (m + 1) + j;
        for (var i = n - 1; i >= 0; i--)
            for (var j = m - 1; j >= 0; j--)
                lcs[Index(i, j)] = string.Equals(a[i], b[j], StringComparison.Ordinal)
                    ? lcs[Index(i + 1, j + 1)] + 1
                    : Math.Max(lcs[Index(i + 1, j)], lcs[Index(i, j + 1)]);

        var rows = new List<SideBySideLine>();
        var removed = new List<int>();
        var added = new List<int>();

        void FlushChanges()
        {
            var count = Math.Max(removed.Count, added.Count);
            for (var k = 0; k < count; k++)
            {
                var hasLeft = k < removed.Count;
                var hasRight = k < added.Count;
                var kind = hasLeft && hasRight ? DiffKind.Changed : hasLeft ? DiffKind.Removed : DiffKind.Added;
                rows.Add(new SideBySideLine(
                    kind,
                    hasLeft ? removed[k] + 1 : null,
                    hasLeft ? a[removed[k]] : null,
                    hasRight ? added[k] + 1 : null,
                    hasRight ? b[added[k]] : null));
            }

            removed.Clear();
            added.Clear();
        }

        int x = 0, y = 0;
        while (x < n || y < m)
        {
            if (x < n && y < m && string.Equals(a[x], b[y], StringComparison.Ordinal))
            {
                FlushChanges();
                rows.Add(new SideBySideLine(DiffKind.Same, x + 1, a[x], y + 1, b[y]));
                x++;
                y++;
            }
            else if (y < m && (x >= n || lcs[Index(x, y + 1)] >= lcs[Index(x + 1, y)]))
            {
                added.Add(y++);
            }
            else
            {
                removed.Add(x++);
            }
        }

        FlushChanges();
        return rows;
    }

    private static List<HeaderDifference> CompareHeaders(IReadOnlyList<KeyValue> previous, IReadOnlyList<KeyValue> current, List<string> masks)
    {
        var before = Joined(previous);
        var after = Joined(current);
        var names = before.Keys.Concat(after.Keys.Where(k => !before.ContainsKey(k))).ToList();

        return names.Select(name =>
        {
            var hidden = SecretRules.IsSensitiveHeader(name);
            var hasBefore = before.TryGetValue(name, out var oldValue);
            var hasAfter = after.TryGetValue(name, out var newValue);
            if (hidden)
                return new HeaderDifference(name,
                    hasBefore ? SecretRules.HiddenValue : null,
                    hasAfter ? SecretRules.HiddenValue : null,
                    Hidden: true);

            return new HeaderDifference(name,
                hasBefore ? Mask(oldValue!, masks) : null,
                hasAfter ? Mask(newValue!, masks) : null,
                Hidden: false);
        }).ToList();
    }

    /// <summary>Header values by name, ignoring case. Repeated headers are joined with commas, as HTTP allows.</summary>
    private static Dictionary<string, string> Joined(IReadOnlyList<KeyValue> headers)
    {
        var values = new Dictionary<string, List<string>>(StringComparer.OrdinalIgnoreCase);
        var order = new List<string>();
        foreach (var header in headers)
        {
            if (!values.TryGetValue(header.Key, out var list))
            {
                list = [];
                values[header.Key] = list;
                order.Add(header.Key);
            }

            list.Add(header.Value);
        }

        return order.ToDictionary(name => name, name => string.Join(", ", values[name]), StringComparer.OrdinalIgnoreCase);
    }

    private static string Mask(string text, List<string> masks) =>
        masks.Aggregate(text, (current, secret) => current.Replace(secret, SecretRules.HiddenValue, StringComparison.Ordinal));
}
