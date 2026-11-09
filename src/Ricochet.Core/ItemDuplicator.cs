namespace Ricochet.Core;

/// <summary>The copy made by <see cref="ItemDuplicator.Duplicate"/>, and the Collection that now holds it.</summary>
public sealed record DuplicateResult(Collection Collection, CollectionItem Copy);

/// <summary>
/// Duplicates a Request or Folder next to the original (ADR 0059). The copy is a full copy with new ids, so it is a
/// new file in the Workspace. Secret values are not moved: a Secret Variable keeps its name, and the copy's Request
/// auth values are kept in memory, so the next save stores them under the copy's own id.
/// </summary>
public static class ItemDuplicator
{
    public const string CopySuffix = " (copy)";

    /// <summary>"Name (copy)", then "Name (copy 2)", "Name (copy 3)" and so on: the first that no sibling already uses.</summary>
    public static string CopyName(string name, IEnumerable<string> siblingNames)
    {
        var taken = siblingNames.ToHashSet(StringComparer.OrdinalIgnoreCase);
        var first = $"{name}{CopySuffix}";
        if (!taken.Contains(first))
            return first;

        for (var n = 2; ; n++)
        {
            var candidate = $"{name} (copy {n})";
            if (!taken.Contains(candidate))
                return candidate;
        }
    }

    /// <summary>
    /// Returns the Collection with a copy placed straight after the item with <paramref name="itemId"/>, or null when
    /// no item has that id. A copy of a hidden item (ADR 0032) is hidden too, so duplicating never makes it run.
    /// </summary>
    public static DuplicateResult? Duplicate(Collection collection, string itemId)
    {
        var newIds = new List<(string From, string To)>();
        var items = InsertCopy(collection.Items, itemId, newIds, out var copy);
        if (items is null || copy is null)
            return null;

        var hidden = collection.HiddenItemIds ?? [];
        var hiddenSet = hidden.ToHashSet();
        var copiedHidden = newIds.Where(p => hiddenSet.Contains(p.From)).Select(p => p.To);
        var allHidden = hidden.Concat(copiedHidden).ToList();

        return new DuplicateResult(collection with
        {
            Items = items,
            HiddenItemIds = allHidden.Count > 0 ? allHidden : null,
        }, copy);
    }

    private static IReadOnlyList<CollectionItem>? InsertCopy(IReadOnlyList<CollectionItem> siblings, string itemId,
        List<(string From, string To)> newIds, out CollectionItem? copy)
    {
        copy = null;
        var index = -1;
        for (var i = 0; i < siblings.Count; i++)
        {
            if (siblings[i].Id == itemId)
            {
                index = i;
                break;
            }
        }

        if (index >= 0)
        {
            var original = siblings[index];
            copy = Clone(original, newIds, CopyName(original.Name, siblings.Select(s => s.Name)));
            var list = siblings.ToList();
            list.Insert(index + 1, copy);
            return list;
        }

        for (var i = 0; i < siblings.Count; i++)
        {
            if (siblings[i].Children.Count == 0)
                continue;

            var children = InsertCopy(siblings[i].Children, itemId, newIds, out copy);
            if (children is null)
                continue;

            var list = siblings.ToList();
            list[i] = siblings[i] with { Children = children };
            return list;
        }

        return null;
    }

    /// <summary>Copies an item and everything under it with new ids. Only the top of the copy takes <paramref name="name"/>.</summary>
    private static CollectionItem Clone(CollectionItem item, List<(string From, string To)> newIds, string? name = null)
    {
        var id = NewId();
        newIds.Add((item.Id, id));
        var itemName = name ?? item.Name;

        return item with
        {
            Id = id,
            Name = itemName,
            Request = item.Request is { } request
                ? request with { Id = NewId(), Name = name ?? request.Name }
                : null,
            Children = item.Children.Select(child => Clone(child, newIds)).ToList(),
        };
    }

    private static string NewId() => Guid.NewGuid().ToString("N");
}
