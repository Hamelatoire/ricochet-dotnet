namespace Ricochet.Core;

/// <summary>
/// Tags on Folders (ADR 0030). A Request has the tags of every Folder it sits in. Tags are compared without regard to case.
/// </summary>
public static class FolderTags
{
    /// <summary>Trims each tag, drops blanks and repeats (ignoring case), and keeps the first spelling. Null when none are left, so an empty list adds nothing to a file.</summary>
    public static IReadOnlyList<string>? Normalize(IEnumerable<string>? tags)
    {
        if (tags is null)
            return null;

        var result = new List<string>();
        foreach (var tag in tags)
        {
            var trimmed = tag.Trim();
            if (trimmed.Length > 0 && !result.Any(t => t.Equals(trimmed, StringComparison.OrdinalIgnoreCase)))
                result.Add(trimmed);
        }

        return result.Count > 0 ? result : null;
    }

    /// <summary>Splits text typed by a person, which separates tags with commas.</summary>
    public static IReadOnlyList<string> Parse(string text) => Normalize(text.Split(',')) ?? [];

    public static bool Contains(IEnumerable<string>? tags, string tag) =>
        tags is not null && tags.Any(t => t.Equals(tag, StringComparison.OrdinalIgnoreCase));

    /// <summary>
    /// Whether a Request with these tags is selected. Include: it must have at least one of the included tags (none means all).
    /// Exclude: it must have none of the excluded tags. Exclusion wins over inclusion.
    /// </summary>
    public static bool Selects(IEnumerable<string> tags, IReadOnlyList<string> include, IReadOnlyList<string> exclude)
    {
        var list = tags.ToList();
        if (include.Count > 0 && !include.Any(t => Contains(list, t)))
            return false;

        return !exclude.Any(t => Contains(list, t));
    }
}
