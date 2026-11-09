//! Port of `NameFilter.cs` (ADR 0050). The name filter on the Collection tree: a Folder or Request is shown
//! when its own name matches, or when something inside it does, so every match keeps its parents. Matching
//! ignores case. Hidden children (ADR 0032) must be left out of `children_shown` by the caller.

/// The query as typed, trimmed. Empty means the filter is off.
pub fn normalize(query: &str) -> &str {
    query.trim()
}

/// Whether a name contains the query. An empty query matches everything.
pub fn matches(name: &str, query: &str) -> bool {
    let query = normalize(query);
    query.is_empty() || name.to_lowercase().contains(&query.to_lowercase())
}

/// Whether a node is shown: its name matches, or any child is shown. Children are checked lazily, stopping at
/// the first one shown.
pub fn shows(name: &str, query: &str, children_shown: impl IntoIterator<Item = bool>) -> bool {
    matches(name, query) || children_shown.into_iter().any(|shown| shown)
}
