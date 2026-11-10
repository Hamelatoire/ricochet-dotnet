//! Port of `CommandPalette.cs` (ADR 0040). Filters the palette's entries as the user types.
//!
//! Matching lowercases both sides with Unicode `to_lowercase`, where .NET uses `OrdinalIgnoreCase`. The two agree
//! on the names the palette holds (ASCII command and Request names).

/// Keeps the entries whose name contains every word of the query, ignoring case. Entries whose name starts with
/// the first word come first, and otherwise the original order is kept. An empty query keeps everything.
///
/// Words are split on whitespace, as the C# version splits on `null` separators.
pub fn filter<T, F>(entries: impl IntoIterator<Item = T>, query: &str, name: F) -> Vec<T>
where
    F: Fn(&T) -> &str,
{
    let all: Vec<T> = entries.into_iter().collect();
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let Some(first) = words.first() else {
        return all;
    };

    let mut matched: Vec<T> = all
        .into_iter()
        .filter(|entry| {
            let name = name(entry).to_lowercase();
            words.iter().all(|word| name.contains(word.as_str()))
        })
        .collect();

    // `sort_by_key` is stable, so entries keep their original order within each group.
    matched.sort_by_key(|entry| !name(entry).to_lowercase().starts_with(first.as_str()));
    matched
}
