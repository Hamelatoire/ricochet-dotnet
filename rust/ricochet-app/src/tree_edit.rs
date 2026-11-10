//! Changes the sidebar makes to the tree: a new Request or Folder, a copy, and a delete. Plain functions on the Core
//! model, so tests reach them without a window. Each returns the id of what it made, which the window then selects.

use std::collections::HashSet;

use ricochet_core::duplicate::duplicate;
use ricochet_core::models::{ApiRequest, Collection, CollectionItem, DEFAULT_COLLECTION_NAME, Workspace};

/// The name a new Request starts with, before the user renames it.
pub const NEW_REQUEST_NAME: &str = "New request";
/// The name a new Folder starts with, before the user renames it.
pub const NEW_FOLDER_NAME: &str = "New folder";

/// `name`, or `name 2`, `name 3` and so on, whichever no sibling has yet. Case is ignored, as in the C# app.
pub fn unique_name<'a>(name: &str, siblings: impl Iterator<Item = &'a str>) -> String {
    let taken: HashSet<String> = siblings.map(str::to_lowercase).collect();
    if !taken.contains(&name.to_lowercase()) {
        return name.to_string();
    }
    (2..).map(|n| format!("{name} {n}")).find(|candidate| !taken.contains(&candidate.to_lowercase())).unwrap()
}

/// Adds an empty GET Request and returns its item id. It goes inside `near` when that is a Collection or Folder,
/// straight after it when it is a Request, and otherwise at the end of the first Collection. A Workspace with no
/// Collection gets the default one, as the C# app does.
pub fn new_request(workspace: &mut Workspace, near: Option<&str>) -> String {
    let make = |siblings: &[CollectionItem]| {
        let name = unique_name(NEW_REQUEST_NAME, siblings.iter().map(|item| item.name.as_str()));
        CollectionItem {
            name: name.clone(),
            request: Some(ApiRequest { name, ..Default::default() }),
            ..Default::default()
        }
    };
    insert(workspace, near, make)
}

/// Adds an empty Folder inside the Collection or Folder `parent`, and returns its id. Returns None, and changes
/// nothing, when `parent` is a Request or is not found.
pub fn new_folder(workspace: &mut Workspace, parent: &str) -> Option<String> {
    let make = |siblings: &[CollectionItem]| CollectionItem {
        name: unique_name(NEW_FOLDER_NAME, siblings.iter().map(|item| item.name.as_str())),
        ..Default::default()
    };
    if let Some(collection) = workspace.collections.iter_mut().find(|collection| collection.id == parent) {
        let folder = make(&collection.items);
        let id = folder.id.clone();
        collection.items.push(folder);
        return Some(id);
    }
    let target = workspace.collections.iter_mut().find_map(|collection| find_item_mut(&mut collection.items, parent))?;
    if target.request.is_some() {
        return None;
    }
    let folder = make(&target.children);
    let id = folder.id.clone();
    target.children.push(folder);
    Some(id)
}

/// Copies a Request or Folder straight after itself (ADR 0059) and returns the copy's id. Collections are not copied.
pub fn duplicate_item(workspace: &mut Workspace, item_id: &str) -> Option<String> {
    let index = workspace.collections.iter().position(|collection| contains(&collection.items, item_id))?;
    let result = duplicate(&workspace.collections[index], item_id)?;
    workspace.collections[index] = result.collection;
    Some(result.copy.id)
}

/// Removes a Collection, Folder or Request with everything inside it. Returns the ids of every removed item, the
/// given one first, so the window can close their tabs. Empty when nothing has that id.
pub fn delete_item(workspace: &mut Workspace, item_id: &str) -> Vec<String> {
    if let Some(index) = workspace.collections.iter().position(|collection| collection.id == item_id) {
        let collection = workspace.collections.remove(index);
        let mut removed = vec![collection.id];
        collection.items.iter().for_each(|item| collect_ids(item, &mut removed));
        return removed;
    }
    for collection in workspace.collections.iter_mut() {
        if let Some(item) = remove_from(&mut collection.items, item_id) {
            let mut removed = Vec::new();
            collect_ids(&item, &mut removed);
            if let Some(hidden) = collection.hidden_item_ids.as_mut() {
                hidden.retain(|id| !removed.contains(id));
                if hidden.is_empty() {
                    collection.hidden_item_ids = None;
                }
            }
            return removed;
        }
    }
    Vec::new()
}

fn insert(workspace: &mut Workspace, near: Option<&str>, make: impl Fn(&[CollectionItem]) -> CollectionItem) -> String {
    if let Some(near) = near {
        if let Some(collection) = workspace.collections.iter_mut().find(|collection| collection.id == near) {
            return push(&mut collection.items, &make);
        }
        for collection in workspace.collections.iter_mut() {
            if let Some(id) = insert_near(&mut collection.items, near, &make) {
                return id;
            }
        }
    }
    if workspace.collections.is_empty() {
        workspace.collections.push(Collection { name: DEFAULT_COLLECTION_NAME.to_string(), ..Default::default() });
    }
    push(&mut workspace.collections[0].items, &make)
}

fn push(siblings: &mut Vec<CollectionItem>, make: &impl Fn(&[CollectionItem]) -> CollectionItem) -> String {
    let item = make(siblings);
    let id = item.id.clone();
    siblings.push(item);
    id
}

/// Inside `near` when it is a Folder, after it when it is a Request. None when `near` is not under these items.
fn insert_near(
    siblings: &mut Vec<CollectionItem>,
    near: &str,
    make: &impl Fn(&[CollectionItem]) -> CollectionItem,
) -> Option<String> {
    if let Some(index) = siblings.iter().position(|item| item.id == near) {
        if siblings[index].request.is_none() {
            return Some(push(&mut siblings[index].children, make));
        }
        let item = make(siblings);
        let id = item.id.clone();
        siblings.insert(index + 1, item);
        return Some(id);
    }
    siblings.iter_mut().find_map(|item| insert_near(&mut item.children, near, make))
}

fn find_item_mut<'a>(items: &'a mut [CollectionItem], item_id: &str) -> Option<&'a mut CollectionItem> {
    for item in items {
        if item.id == item_id {
            return Some(item);
        }
        if let Some(found) = find_item_mut(&mut item.children, item_id) {
            return Some(found);
        }
    }
    None
}

fn contains(items: &[CollectionItem], item_id: &str) -> bool {
    items.iter().any(|item| item.id == item_id || contains(&item.children, item_id))
}

fn remove_from(siblings: &mut Vec<CollectionItem>, item_id: &str) -> Option<CollectionItem> {
    if let Some(index) = siblings.iter().position(|item| item.id == item_id) {
        return Some(siblings.remove(index));
    }
    siblings.iter_mut().find_map(|item| remove_from(&mut item.children, item_id))
}

fn collect_ids(item: &CollectionItem, ids: &mut Vec<String>) {
    ids.push(item.id.clone());
    item.children.iter().for_each(|child| collect_ids(child, ids));
}

/// Renames a Collection or Folder. A Request is renamed with [`crate::edit::rename_request`], which also sets the
/// name the Request holds. Returns false, and changes nothing, when nothing has that id, it is a Request, or the name
/// is the same.
pub fn rename_container(workspace: &mut Workspace, item_id: &str, name: &str) -> bool {
    if let Some(collection) = workspace.collections.iter_mut().find(|collection| collection.id == item_id) {
        if collection.name == name {
            return false;
        }
        collection.name = name.to_string();
        return true;
    }
    let target = workspace.collections.iter_mut().find_map(|collection| find_item_mut(&mut collection.items, item_id));
    match target {
        Some(item) if item.request.is_none() && item.name != name => {
            item.name = name.to_string();
            true
        }
        _ => false,
    }
}

/// Whether the item itself is hidden (ADR 0032). False for a Collection, which is never hidden.
pub fn is_hidden(workspace: &Workspace, item_id: &str) -> bool {
    workspace
        .collections
        .iter()
        .any(|collection| collection.hidden_item_ids.as_deref().is_some_and(|ids| ids.iter().any(|id| id == item_id)))
}

/// Hides or shows a Folder or Request. Returns false, and changes nothing, for a Collection, an unknown id, or an
/// item already in that state. The list stays None when empty, so a Collection's file gains nothing.
pub fn set_hidden(workspace: &mut Workspace, item_id: &str, hidden: bool) -> bool {
    let Some(collection) = workspace.collections.iter_mut().find(|collection| contains(&collection.items, item_id)) else {
        return false;
    };
    let mut ids = collection.hidden_item_ids.take().unwrap_or_default();
    let was = ids.iter().any(|id| id == item_id);
    if hidden && !was {
        ids.push(item_id.to_string());
    } else if !hidden && was {
        ids.retain(|id| id != item_id);
    } else {
        collection.hidden_item_ids = if ids.is_empty() { None } else { Some(ids) };
        return false;
    }
    collection.hidden_item_ids = if ids.is_empty() { None } else { Some(ids) };
    true
}

/// How many items in the Workspace are hidden.
pub fn hidden_count(workspace: &Workspace) -> usize {
    workspace.collections.iter().map(|collection| collection.hidden_item_ids.as_deref().map_or(0, <[String]>::len)).sum()
}

/// Where a dropped item goes, relative to the item it was dropped on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drop {
    /// Just before the target, among its siblings.
    Before,
    /// Just after the target, among its siblings.
    After,
    /// At the end of the target, which must be a Collection or Folder.
    Into,
}

/// Moves the item with `source` to where it is dropped on `target`. Returns false, and changes nothing, when the drop
/// is not allowed. A Collection stays at the top level and can only be dropped before or after another Collection.
/// An item cannot be dropped on itself, into one of its own Folders, or into a Request.
pub fn move_item(workspace: &mut Workspace, source: &str, target: &str, drop: Drop) -> bool {
    if source == target {
        return false;
    }
    if let Some(source_index) = workspace.collections.iter().position(|c| c.id == source) {
        let Some(target_index) = workspace.collections.iter().position(|c| c.id == target) else {
            return false;
        };
        if drop == Drop::Into {
            return false;
        }
        let collection = workspace.collections.remove(source_index);
        let index = if source_index < target_index { target_index - 1 } else { target_index };
        workspace.collections.insert(if drop == Drop::After { index + 1 } else { index }, collection);
        return true;
    }

    // Every check happens before anything is taken out, so a refused drop changes nothing.
    let Some(source_item) = workspace.collections.iter().find_map(|c| find_in(&c.items, source)) else {
        return false;
    };
    if contains(&source_item.children, target) {
        return false;
    }
    let Some(target_item) = workspace.collections.iter().find_map(|c| find_in(&c.items, target)) else {
        let target_is_collection = workspace.collections.iter().any(|c| c.id == target);
        if !target_is_collection || drop == Drop::Before || drop == Drop::After {
            return false;
        }
        return move_into_collection(workspace, source, target);
    };
    if drop == Drop::Into && target_item.request.is_some() {
        return false;
    }

    let Some(moving) = take_item(workspace, source) else {
        return false;
    };
    let placed = place(workspace, target, drop, moving);
    debug_assert!(placed, "the target was checked above");
    true
}

fn move_into_collection(workspace: &mut Workspace, source: &str, target: &str) -> bool {
    let Some(moving) = take_item(workspace, source) else {
        return false;
    };
    match workspace.collections.iter_mut().find(|c| c.id == target) {
        Some(collection) => {
            collection.items.push(moving);
            true
        }
        None => false,
    }
}

fn find_in<'a>(items: &'a [CollectionItem], id: &str) -> Option<&'a CollectionItem> {
    for item in items {
        if item.id == id {
            return Some(item);
        }
        if let Some(found) = find_in(&item.children, id) {
            return Some(found);
        }
    }
    None
}

/// Takes the item out of the workspace, wherever it is inside a Collection.
fn take_item(workspace: &mut Workspace, id: &str) -> Option<CollectionItem> {
    workspace.collections.iter_mut().find_map(|collection| remove_from(&mut collection.items, id))
}

/// Places the item next to or inside the target. Returns false when the target is not found.
fn place(workspace: &mut Workspace, target: &str, drop: Drop, item: CollectionItem) -> bool {
    let mut item = Some(item);
    for collection in workspace.collections.iter_mut() {
        if let Some(done) = place_in(&mut collection.items, target, drop, &mut item) {
            return done;
        }
    }
    false
}

/// `Some(true)` when placed here, `Some(false)` when the target is here but cannot take the item, `None` when the
/// target is not under these siblings. The item is taken out of `item` when it is placed.
fn place_in(siblings: &mut Vec<CollectionItem>, target: &str, drop: Drop, item: &mut Option<CollectionItem>) -> Option<bool> {
    if let Some(index) = siblings.iter().position(|s| s.id == target) {
        let Some(moving) = item.take() else {
            return Some(false);
        };
        match drop {
            Drop::Before => siblings.insert(index, moving),
            Drop::After => siblings.insert(index + 1, moving),
            Drop::Into => siblings[index].children.push(moving),
        }
        return Some(true);
    }
    for sibling in siblings.iter_mut() {
        if let Some(done) = place_in(&mut sibling.children, target, drop, item) {
            return Some(done);
        }
    }
    None
}
