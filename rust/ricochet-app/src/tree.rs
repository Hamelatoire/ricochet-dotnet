//! The sidebar tree as plain rows, built from a Workspace. No window is needed, so tests cover it directly.

use std::collections::BTreeSet;

use ricochet_core::models::{ApiRequest, CollectionItem, Workspace};

/// What a sidebar row stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Collection,
    Folder,
    Request,
}

/// One line of the sidebar, in display order. `depth` is 0 for a Collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    pub id: String,
    pub depth: usize,
    pub kind: RowKind,
    pub label: String,
    /// The HTTP method of a Request. None for Collections and Folders.
    pub method: Option<String>,
    /// True for an item the user hid (ADR 0032), or one inside a hidden Folder.
    pub hidden: bool,
}

/// Flattens the Collections into rows, depth first, in the order the Workspace holds them.
///
/// Hidden items and everything inside them are left out unless `show_hidden` is set (ADR 0032).
pub fn build_tree(workspace: &Workspace, show_hidden: bool) -> Vec<TreeRow> {
    build_tree_folded(workspace, show_hidden, &BTreeSet::new())
}

/// As [`build_tree`], but leaves out everything inside a Collection or Folder whose id is in `collapsed`.
/// The folded item itself keeps its row.
pub fn build_tree_folded(workspace: &Workspace, show_hidden: bool, collapsed: &BTreeSet<String>) -> Vec<TreeRow> {
    let mut rows = Vec::new();
    for collection in &workspace.collections {
        rows.push(TreeRow {
            id: collection.id.clone(),
            depth: 0,
            kind: RowKind::Collection,
            label: collection.name.clone(),
            method: None,
            hidden: false,
        });
        let hidden_ids = collection.hidden_item_ids.as_deref().unwrap_or(&[]);
        if !collapsed.contains(&collection.id) {
            push_items(&collection.items, 1, hidden_ids, show_hidden, false, collapsed, &mut rows);
        }
    }
    rows
}

fn push_items(
    items: &[CollectionItem],
    depth: usize,
    hidden_ids: &[String],
    show_hidden: bool,
    parent_hidden: bool,
    collapsed: &BTreeSet<String>,
    rows: &mut Vec<TreeRow>,
) {
    for item in items {
        let hidden = parent_hidden || hidden_ids.contains(&item.id);
        if hidden && !show_hidden {
            continue;
        }
        let (kind, method) = match &item.request {
            Some(request) => (RowKind::Request, Some(request.method.clone())),
            None => (RowKind::Folder, None),
        };
        rows.push(TreeRow { id: item.id.clone(), depth, kind, label: item.name.clone(), method, hidden });
        if !collapsed.contains(&item.id) {
            push_items(&item.children, depth + 1, hidden_ids, show_hidden, hidden, collapsed, rows);
        }
    }
}

/// The Request with this id, wherever it sits in the Collections.
pub fn find_request<'a>(workspace: &'a Workspace, id: &str) -> Option<&'a ApiRequest> {
    workspace.collections.iter().find_map(|collection| find_in(&collection.items, id))
}

fn find_in<'a>(items: &'a [CollectionItem], id: &str) -> Option<&'a ApiRequest> {
    items.iter().find_map(|item| {
        if item.id == id {
            if let Some(request) = &item.request {
                return Some(request);
            }
        }
        find_in(&item.children, id)
    })
}
