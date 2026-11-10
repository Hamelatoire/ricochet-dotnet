//! Port of src/Ricochet.Core/FolderTags.cs (ADR 0030). A Request has the tags of every Folder it sits in.
//! Tags are compared without regard to case.

/// Trims each tag, drops blanks and repeats (ignoring case), and keeps the first spelling.
/// Returns `None` when the input is absent or no tags are left, so an empty list adds nothing to a file.
pub fn normalize<I, S>(tags: Option<I>) -> Option<Vec<String>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut result: Vec<String> = Vec::new();
    for tag in tags? {
        let trimmed = tag.as_ref().trim();
        if !trimmed.is_empty() && !result.iter().any(|kept| same_tag(kept, trimmed)) {
            result.push(trimmed.to_string());
        }
    }

    if result.is_empty() { None } else { Some(result) }
}

/// Splits text typed by a person, which separates tags with commas.
pub fn parse(text: &str) -> Vec<String> {
    normalize(Some(text.split(','))).unwrap_or_default()
}

/// Whether `tags` holds `tag`, ignoring case.
pub fn contains<S: AsRef<str>>(tags: &[S], tag: &str) -> bool {
    tags.iter().any(|t| same_tag(t.as_ref(), tag))
}

/// Whether a Request with these tags is selected.
///
/// Include: it must have at least one of the included tags (an empty list means all).
/// Exclude: it must have none of the excluded tags. Exclusion wins over inclusion.
pub fn selects<S, I, E>(tags: &[S], include: &[I], exclude: &[E]) -> bool
where
    S: AsRef<str>,
    I: AsRef<str>,
    E: AsRef<str>,
{
    if !include.is_empty() && !include.iter().any(|t| contains(tags, t.as_ref())) {
        return false;
    }

    !exclude.iter().any(|t| contains(tags, t.as_ref()))
}

/// Case-insensitive comparison in the same spirit as C#'s `StringComparison.OrdinalIgnoreCase`,
/// which compares upper-cased characters.
fn same_tag(a: &str, b: &str) -> bool {
    a.to_uppercase() == b.to_uppercase()
}
