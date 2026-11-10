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
