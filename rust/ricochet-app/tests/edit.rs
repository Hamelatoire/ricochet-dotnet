//! Tests for the edits the window makes to a Request, and for saving them through the store.

use std::time::{SystemTime, UNIX_EPOCH};

use pretty_assertions::assert_eq;
use ricochet_app::edit::{Field, add_header, cycle_auth_kind, cycle_method, find_request_mut, next_method, remove_header, set_field, toggle_header, toggle_key_in_query};
use ricochet_app::shell::ShellModel;
use ricochet_app::state::AppState;
use ricochet_core::models::{ApiRequest, Collection, CollectionItem, KeyValue, RequestAuth, Workspace, auth_kinds};
use ricochet_core::store::WorkspaceFolder;

fn request() -> ApiRequest {
    ApiRequest {
        url: "https://a.test".into(),
        headers: vec![KeyValue::new("Accept", "text/plain"), KeyValue { key: "X-Key".into(), value: String::new(), is_secret: true, ..Default::default() }],
        ..Default::default()
    }
}

fn workspace() -> Workspace {
    let inner = CollectionItem { id: "inner".into(), name: "Inner".into(), request: Some(request()), ..Default::default() };
    let folder = CollectionItem { id: "folder".into(), name: "Folder".into(), children: vec![inner], ..Default::default() };
    Workspace { collections: vec![Collection { id: "c".into(), name: "C".into(), items: vec![folder], ..Default::default() }], ..Default::default() }
}

#[test]
fn methods_step_in_order_and_wrap() {
    assert_eq!(next_method("GET"), "POST");
    assert_eq!(next_method("options"), "GET");
    assert_eq!(next_method("PROPFIND"), "GET");
    let mut request = request();
    cycle_method(&mut request);
    assert_eq!(request.method, "POST");
}

#[test]
fn setting_a_field_reports_whether_anything_changed() {
    let mut request = request();
    assert!(set_field(&mut request, Field::Url, "https://b.test"));
    assert!(!set_field(&mut request, Field::Url, "https://b.test"));
    assert!(set_field(&mut request, Field::HeaderKey(0), "Content-Type"));
    assert!(set_field(&mut request, Field::HeaderValue(0), "application/json"));
    assert_eq!(request.url, "https://b.test");
    assert_eq!(request.headers[0], KeyValue::new("Content-Type", "application/json"));
    assert!(!set_field(&mut request, Field::HeaderValue(9), "x"));
}

#[test]
fn a_secret_headers_value_cannot_be_typed_into_the_request() {
    let mut request = request();
    assert!(!set_field(&mut request, Field::HeaderValue(1), "hunter2"));
    assert_eq!(request.headers[1].value, "");
    assert!(set_field(&mut request, Field::HeaderKey(1), "X-Api-Key"));
}

#[test]
fn headers_can_be_added_removed_and_turned_off() {
    let mut request = request();
    assert_eq!(add_header(&mut request), 2);
    assert_eq!(request.headers[2], KeyValue::default());
    assert!(toggle_header(&mut request, 0));
    assert!(!request.headers[0].enabled);
    assert!(remove_header(&mut request, 0));
    assert_eq!(request.headers.len(), 2);
    assert!(!remove_header(&mut request, 5));
    assert!(!toggle_header(&mut request, 5));
}

#[test]
fn a_request_is_found_for_editing_by_its_items_id() {
    let mut workspace = workspace();
    assert!(find_request_mut(&mut workspace, "folder").is_none());
    assert!(find_request_mut(&mut workspace, "missing").is_none());
    find_request_mut(&mut workspace, "inner").unwrap().url = "https://changed.test".into();
    assert_eq!(workspace.collections[0].items[0].children[0].request.as_ref().unwrap().url, "https://changed.test");
}

#[test]
fn unsaved_marks_are_kept_per_request_and_cleared_together() {
    let mut state = AppState::default();
    assert!(!state.has_unsaved());
    state.mark_unsaved("a");
    state.mark_unsaved("b");
    state.mark_unsaved("a");
    assert!(state.is_unsaved("a"));
    assert_eq!(state.unsaved_count(), 2);
    state.clear_unsaved();
    assert!(!state.has_unsaved());
}

fn temp_folder(name: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let path = std::env::temp_dir().join(format!("ricochet-edit-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn an_edit_saved_through_the_opened_folder_is_read_back_and_touches_one_file() {
    let folder = temp_folder("save");
    write_fixture(&folder);

    let (mut model, store) = ShellModel::open(Some(folder.clone()));
    let mut store = store.expect("the folder opened");
    let before: Vec<_> = walk(&folder);

    assert!(set_field(find_request_mut(&mut model.workspace, "inner").unwrap(), Field::Url, "https://edited.test"));
    assert!(!store.has_outside_changes());
    assert_eq!(store.save(&model.workspace).unwrap(), vec![]);

    let (reloaded, _) = WorkspaceFolder::new(&folder).load().unwrap();
    assert_eq!(reloaded.collections[0].items[0].children[0].request.as_ref().unwrap().url, "https://edited.test");

    let after = walk(&folder);
    let changed: Vec<_> = before.iter().zip(&after).filter(|(a, b)| a != b).collect();
    assert_eq!(before.len(), after.len());
    let names: Vec<String> = changed.iter().map(|((path, _), _)| path.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(names, vec!["inner-inner.json"], "only the edited Request's file should differ");
    std::fs::remove_dir_all(&folder).ok();
}

#[test]
fn a_change_made_outside_the_app_is_noticed_before_saving() {
    let folder = temp_folder("outside");
    write_fixture(&folder);
    let (_, store) = ShellModel::open(Some(folder.clone()));
    let store = store.unwrap();
    assert!(!store.has_outside_changes());
    std::fs::write(folder.join("collections").join("extra.json"), "{}").unwrap();
    assert!(store.has_outside_changes());
    std::fs::remove_dir_all(&folder).ok();
}

/// Writes the fixture as the app would: the store loads first, which gives the Workspace its id, and then saves.
fn write_fixture(folder: &std::path::Path) {
    let mut store = WorkspaceFolder::new(folder);
    store.load().unwrap();
    store.save(&workspace()).unwrap();
}

/// Every file under a folder with its text, sorted by path.
fn walk(root: &std::path::Path) -> Vec<(std::path::PathBuf, String)> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push((path.clone(), std::fs::read_to_string(&path).unwrap_or_default()));
            }
        }
    }
    files.sort();
    files
}

#[test]
fn the_auth_kind_steps_through_the_offered_kinds() {
    let mut request = request();
    assert!(request.auth.is_none());
    let mut seen = Vec::new();
    for _ in 0..5 {
        cycle_auth_kind(&mut request);
        seen.push(request.auth.as_ref().unwrap().kind.clone());
    }
    assert_eq!(seen, vec!["bearer", "basic", "apikey", "none", "bearer"]);
}

#[test]
fn a_signing_kind_moves_to_the_first_offered_kind() {
    let mut request = request();
    request.auth = Some(RequestAuth { kind: auth_kinds::AWS_SIG_V4.to_string(), ..Default::default() });
    cycle_auth_kind(&mut request);
    assert_eq!(request.auth.unwrap().kind, auth_kinds::NONE);
}

#[test]
fn the_username_and_key_name_are_editable_and_create_the_settings_when_missing() {
    let mut request = request();
    assert!(set_field(&mut request, Field::AuthUsername, "alice"));
    assert!(set_field(&mut request, Field::AuthKeyName, "X-Api-Key"));
    let auth = request.auth.unwrap();
    assert_eq!((auth.username.as_str(), auth.key_name.as_str()), ("alice", "X-Api-Key"));
    assert_eq!(auth.kind, auth_kinds::NONE, "editing a field does not change the kind");
}

#[test]
fn the_query_placement_of_an_api_key_can_be_toggled() {
    let mut request = request();
    toggle_key_in_query(&mut request);
    assert!(request.auth.as_ref().unwrap().key_in_query);
    toggle_key_in_query(&mut request);
    assert!(!request.auth.as_ref().unwrap().key_in_query);
}
