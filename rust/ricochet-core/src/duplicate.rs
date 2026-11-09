//! Port of `ItemDuplicator.cs` (ADR 0059). Duplicating a Request or Folder makes a full copy next to the
//! original with new ids, so it is a new file in the Workspace. Copies keep their names unless they are the
//! top of the copy, which gets "Name (copy)" or a number when that is taken among its siblings.
//! Secret values are not moved: a Secret keeps its name, and the copy's auth values are kept in memory.

use std::collections::HashSet;

use crate::models::{ApiRequest, Collection, CollectionItem, new_id};

pub const COPY_SUFFIX: &str = " (copy)";

/// The copy made by [`duplicate`], and the Collection that now holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct DuplicateResult {
    pub collection: Collection,
    pub copy: CollectionItem,
}

/// "Name (copy)", then "Name (copy 2)", "Name (copy 3)" and so on: the first that no sibling already uses.
/// Names are compared ignoring case, as in C#.
pub fn copy_name<S: AsRef<str>>(name: &str, sibling_names: &[S]) -> String {
    let taken: HashSet<String> = sibling_names.iter().map(|s| s.as_ref().to_lowercase()).collect();
    let first = format!("{name}{COPY_SUFFIX}");
    if !taken.contains(&first.to_lowercase()) {
        return first;
    }

    let mut n = 2;
    loop {
        let candidate = format!("{name} (copy {n})");
        if !taken.contains(&candidate.to_lowercase()) {
            return candidate;
        }
        n += 1;
    }
}

/// Returns the Collection with a copy placed straight after the item with `item_id`, or `None` when no item
/// has that id. A copy of a hidden item (ADR 0032) is hidden too, so duplicating never makes it run.
pub fn duplicate(collection: &Collection, item_id: &str) -> Option<DuplicateResult> {
    let mut new_ids = Vec::new();
    let (items, copy) = insert_copy(&collection.items, item_id, &mut new_ids)?;

    let hidden = collection.hidden_item_ids.clone().unwrap_or_default();
    let hidden_set: HashSet<&str> = hidden.iter().map(String::as_str).collect();
    let copied_hidden: Vec<String> = new_ids
        .iter()
        .filter(|(from, _)| hidden_set.contains(from.as_str()))
        .map(|(_, to)| to.clone())
        .collect();

    let mut all_hidden = hidden.clone();
    all_hidden.extend(copied_hidden);

    let mut updated = collection.clone();
    updated.items = items;
    updated.hidden_item_ids = if all_hidden.is_empty() { None } else { Some(all_hidden) };

    Some(DuplicateResult { collection: updated, copy })
}

/// Returns the siblings with a copy inserted after the item, plus that copy, searching nested Folders.
/// Returns `None` when the id is not found under these siblings, so nothing is recorded in `new_ids`.
fn insert_copy(
    siblings: &[CollectionItem],
    item_id: &str,
    new_ids: &mut Vec<(String, String)>,
) -> Option<(Vec<CollectionItem>, CollectionItem)> {
    if let Some(index) = siblings.iter().position(|s| s.id == item_id) {
        let original = &siblings[index];
        let names: Vec<&str> = siblings.iter().map(|s| s.name.as_str()).collect();
        let name = copy_name(&original.name, &names);
        let copy = clone_item(original, new_ids, Some(&name));

        let mut list = siblings.to_vec();
        list.insert(index + 1, copy.clone());
        return Some((list, copy));
    }

    for (i, sibling) in siblings.iter().enumerate() {
        if sibling.children.is_empty() {
            continue;
        }
        if let Some((children, copy)) = insert_copy(&sibling.children, item_id, new_ids) {
            let mut list = siblings.to_vec();
            list[i] = CollectionItem { children, ..sibling.clone() };
            return Some((list, copy));
        }
    }

    None
}

/// Copies an item and everything under it with new ids. Only the top of the copy takes `name`.
fn clone_item(item: &CollectionItem, new_ids: &mut Vec<(String, String)>, name: Option<&str>) -> CollectionItem {
    let id = new_id();
    new_ids.push((item.id.clone(), id.clone()));

    let request = item.request.as_ref().map(|request| ApiRequest {
        id: new_id(),
        name: name.unwrap_or(&request.name).to_string(),
        ..request.clone()
    });

    let children = item.children.iter().map(|child| clone_item(child, new_ids, None)).collect();

    CollectionItem {
        id,
        name: name.unwrap_or(&item.name).to_string(),
        request,
        children,
        scripts: item.scripts.clone(),
        variables: item.variables.clone(),
        tags: item.tags.clone(),
    }
}
