use std::time::{SystemTime, UNIX_EPOCH};

use pretty_assertions::assert_eq;
use ricochet_app::tree::{RowKind, build_tree, find_request};
use ricochet_app::tree_edit::{delete_item, duplicate_item, new_folder, new_request, unique_name};
use ricochet_core::models::{ApiRequest, Collection, CollectionItem, Workspace};
use ricochet_core::store::WorkspaceFolder;

fn request(id: &str, name: &str) -> CollectionItem {
    CollectionItem { id: id.into(), name: name.into(), request: Some(ApiRequest { name: name.into(), ..Default::default() }), ..Default::default() }
}

fn workspace() -> Workspace {
    let folder = CollectionItem { id: "folder".into(), name: "Folder".into(), children: vec![request("inner", "Inner")], ..Default::default() };
    let collection = Collection { id: "c".into(), name: "C".into(), items: vec![request("top", "Top"), folder], ..Default::default() };
    Workspace { collections: vec![collection], ..Default::default() }
}

fn labels(workspace: &Workspace) -> Vec<(usize, String)> {
    build_tree(workspace, true).into_iter().map(|row| (row.depth, row.label)).collect()
}

#[test]
fn names_get_a_number_when_a_sibling_already_has_them() {
    assert_eq!(unique_name("New request", ["Other"].into_iter()), "New request");
    assert_eq!(unique_name("New request", ["new REQUEST", "New request 2"].into_iter()), "New request 3");
}

#[test]
fn a_new_request_goes_after_a_request_inside_a_folder_and_into_a_collection() {
    let mut workspace = workspace();
    let after_top = new_request(&mut workspace, Some("top"));
    let in_folder = new_request(&mut workspace, Some("folder"));
    let in_collection = new_request(&mut workspace, Some("c"));

    assert_eq!(
        labels(&workspace),
        vec![
            (0, "C".to_string()),
            (1, "Top".to_string()),
            (1, "New request".to_string()),
            (1, "Folder".to_string()),
            (2, "Inner".to_string()),
            (2, "New request".to_string()),
            (1, "New request 2".to_string()),
        ]
    );
    for id in [after_top, in_folder, in_collection] {
        let found = find_request(&workspace, &id).expect("the new item is a Request");
        assert_eq!(found.method, "GET");
    }
}

#[test]
fn a_new_request_with_nowhere_to_go_makes_the_default_collection() {
    let mut workspace = Workspace::default();
    let id = new_request(&mut workspace, None);
    assert_eq!(workspace.collections.len(), 1);
    assert_eq!(workspace.collections[0].name, "My Requests");
    assert!(find_request(&workspace, &id).is_some());
}

#[test]
fn a_new_folder_goes_inside_a_collection_or_folder_but_not_a_request() {
    let mut workspace = workspace();
    assert_eq!(new_folder(&mut workspace, "top"), None);
    let id = new_folder(&mut workspace, "folder").unwrap();
    new_folder(&mut workspace, "c").unwrap();
    let rows = build_tree(&workspace, true);
    let row = rows.iter().find(|row| row.id == id).unwrap();
    assert_eq!((row.depth, row.kind, row.label.as_str()), (2, RowKind::Folder, "New folder"));
    assert_eq!(rows.last().unwrap().label, "New folder");
}

#[test]
fn a_copy_sits_after_the_original_with_new_ids() {
    let mut workspace = workspace();
    let copy = duplicate_item(&mut workspace, "folder").unwrap();
    assert_ne!(copy, "folder");
    assert_eq!(
        labels(&workspace),
        vec![
            (0, "C".to_string()),
            (1, "Top".to_string()),
            (1, "Folder".to_string()),
            (2, "Inner".to_string()),
            (1, "Folder (copy)".to_string()),
            (2, "Inner".to_string()),
        ]
    );
    assert_eq!(duplicate_item(&mut workspace, "c"), None, "a Collection is not copied");
}

#[test]
fn a_delete_removes_everything_inside_and_reports_the_ids() {
    let mut workspace = workspace();
    workspace.collections[0].hidden_item_ids = Some(vec!["inner".into()]);
    assert_eq!(delete_item(&mut workspace, "folder"), vec!["folder".to_string(), "inner".to_string()]);
    assert_eq!(workspace.collections[0].hidden_item_ids, None);
    assert_eq!(labels(&workspace), vec![(0, "C".to_string()), (1, "Top".to_string())]);
    assert_eq!(delete_item(&mut workspace, "missing"), Vec::<String>::new());
    assert_eq!(delete_item(&mut workspace, "c"), vec!["c".to_string(), "top".to_string()]);
    assert!(workspace.collections.is_empty());
}

#[test]
fn added_and_deleted_requests_reach_the_folder_on_save() {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let folder = std::env::temp_dir().join(format!("ricochet-tree-edit-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let mut store = WorkspaceFolder::new(&folder);
    store.load().unwrap();
    store.save(&workspace()).unwrap();

    let (mut loaded, _) = store.load().unwrap();
    let added = new_request(&mut loaded, Some("top"));
    delete_item(&mut loaded, "folder");
    store.save(&loaded).unwrap();

    let (reloaded, issues) = WorkspaceFolder::new(&folder).load().unwrap();
    assert!(issues.is_empty());
    assert!(find_request(&reloaded, &added).is_some());
    assert!(find_request(&reloaded, "inner").is_none(), "the deleted Request's file is gone");
    std::fs::remove_dir_all(&folder).ok();
}
