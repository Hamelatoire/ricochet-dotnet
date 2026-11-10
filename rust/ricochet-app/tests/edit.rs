//! Tests for the edits the window makes to a Request, and for saving them through the store.

use std::time::{SystemTime, UNIX_EPOCH};

use pretty_assertions::assert_eq;
use ricochet_app::edit::{Field, set_secret, rename_request, add_header, add_check, cycle_auth_kind, cycle_check_op, cycle_check_source, cycle_method, fill_secret_headers, find_request_mut, next_method, remove_check, remove_header, remove_header_and_secret, set_field, set_header_name, set_secret_header, toggle_check, toggle_header, toggle_key_in_query};
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

#[test]
fn checks_are_added_with_a_default_and_go_back_to_none_when_the_last_is_removed() {
    let mut request = request();
    assert_eq!(add_check(&mut request), 0);
    assert_eq!(add_check(&mut request), 1);
    assert!(remove_check(&mut request, 0));
    assert!(remove_check(&mut request, 0));
    assert!(request.assertions.is_none(), "an empty list is written as none");
    assert!(!remove_check(&mut request, 0));
}

#[test]
fn a_check_source_and_comparison_step_through_their_lists() {
    use ricochet_core::assertions::{ops, sources};
    let mut request = request();
    add_check(&mut request);
    assert!(cycle_check_source(&mut request, 0));
    assert_eq!(request.assertions.as_ref().unwrap()[0].source, sources::TIME);
    assert!(cycle_check_op(&mut request, 0));
    assert_eq!(request.assertions.as_ref().unwrap()[0].op, ops::NOT_EQUAL);
    assert!(!cycle_check_op(&mut request, 4));
}

#[test]
fn a_check_path_and_expected_text_are_editable_and_a_check_can_be_turned_off() {
    let mut request = request();
    add_check(&mut request);
    assert!(set_field(&mut request, Field::CheckPath(0), "Content-Type"));
    assert!(set_field(&mut request, Field::CheckExpected(0), "application/json"));
    assert!(!set_field(&mut request, Field::CheckExpected(3), "x"));
    let check = &request.assertions.as_ref().unwrap()[0];
    assert_eq!((check.path.as_str(), check.expected.as_str()), ("Content-Type", "application/json"));
    assert!(toggle_check(&mut request, 0));
    assert!(!request.assertions.as_ref().unwrap()[0].enabled);
}

#[test]
fn a_rename_sets_the_item_name_and_the_request_name_together() {
    let mut workspace = workspace();
    assert!(rename_request(&mut workspace, "inner", "Inner, renamed"));
    let item = &workspace.collections[0].items[0].children[0];
    assert_eq!(item.name, "Inner, renamed");
    assert_eq!(item.request.as_ref().unwrap().name, "Inner, renamed");
    assert!(!rename_request(&mut workspace, "inner", "Inner, renamed"), "the same name changes nothing");
    assert!(!rename_request(&mut workspace, "folder", "Only a Request can be renamed here"));
    assert!(!rename_request(&mut workspace, "missing", "x"));
}

#[test]
fn a_typed_secret_is_stored_and_an_empty_one_removes_it() {
    use ricochet_core::secrets::{SecretValue, auth_key};
    let mut secrets = std::collections::BTreeMap::new();
    let key = auth_key("r1", "token");
    assert!(set_secret(&mut secrets, &key, "tok-1"));
    assert!(!set_secret(&mut secrets, &key, "tok-1"), "the same value changes nothing");
    assert_eq!(secrets.get(&key), Some(&SecretValue::new("tok-1")));
    assert!(set_secret(&mut secrets, &key, ""));
    assert!(secrets.is_empty());
    assert!(!set_secret(&mut secrets, &key, ""));
}

fn secret_header_request() -> ApiRequest {
    ApiRequest {
        id: "r1".into(),
        headers: vec![KeyValue::new("Accept", "text/plain"), KeyValue { key: "X-Api-Key".into(), value: String::new(), is_secret: true, ..Default::default() }],
        ..Default::default()
    }
}

#[test]
fn a_typed_secret_header_value_goes_to_the_vault_and_the_request_keeps_none() {
    use ricochet_core::secrets::{SecretValue, header_key};
    let mut request = secret_header_request();
    let mut secrets = std::collections::BTreeMap::new();
    assert!(set_secret_header(&mut request, &mut secrets, 1, "k-123"));
    assert_eq!(secrets.get(&header_key("r1", "X-Api-Key")), Some(&SecretValue::new("k-123")));
    assert_eq!(request.headers[1].value, "", "the Workspace keeps no value");
    assert!(!set_secret_header(&mut request, &mut secrets, 1, "k-123"), "the same value changes nothing");
    assert!(set_secret_header(&mut request, &mut secrets, 1, ""));
    assert!(secrets.is_empty());
}

#[test]
fn a_secret_header_value_typed_over_a_value_left_in_the_file_clears_the_file_copy() {
    let mut request = secret_header_request();
    request.headers[1].value = "old-in-file".into();
    let mut secrets = std::collections::BTreeMap::new();
    assert!(set_secret_header(&mut request, &mut secrets, 1, "new"));
    assert_eq!(request.headers[1].value, "");
}

#[test]
fn set_secret_header_leaves_plain_headers_and_missing_indexes_alone() {
    let mut request = secret_header_request();
    let mut secrets = std::collections::BTreeMap::new();
    assert!(!set_secret_header(&mut request, &mut secrets, 0, "x"));
    assert!(!set_secret_header(&mut request, &mut secrets, 9, "x"));
    assert_eq!(request.headers[0].value, "text/plain");
    assert!(secrets.is_empty());
}

#[test]
fn a_secret_headers_key_ignores_case_and_surrounding_space_but_not_the_request() {
    use ricochet_core::secrets::header_key;
    assert_eq!(header_key("r1", " X-API-Key "), "header/r1/x-api-key");
    assert_ne!(header_key("r1", "X-Api-Key"), header_key("r2", "X-Api-Key"));
    assert_ne!(header_key("r1", "X-Api-Key"), ricochet_core::secrets::auth_key("r1", "keyValue"));
}

#[test]
fn renaming_a_secret_header_moves_its_value_with_it() {
    use ricochet_core::secrets::{SecretValue, header_key};
    let mut request = secret_header_request();
    let mut secrets = std::collections::BTreeMap::new();
    set_secret_header(&mut request, &mut secrets, 1, "k-123");
    assert!(set_header_name(&mut request, &mut secrets, 1, "X-Token"));
    assert_eq!(secrets.get(&header_key("r1", "X-Token")), Some(&SecretValue::new("k-123")));
    assert!(!secrets.contains_key(&header_key("r1", "X-Api-Key")));
    assert!(!set_header_name(&mut request, &mut secrets, 1, "X-Token"), "the same name changes nothing");
}

#[test]
fn renaming_a_plain_header_leaves_the_vault_alone() {
    let mut request = secret_header_request();
    let mut secrets = std::collections::BTreeMap::new();
    set_secret_header(&mut request, &mut secrets, 1, "k-123");
    let before = secrets.clone();
    assert!(set_header_name(&mut request, &mut secrets, 0, "Accept-Language"));
    assert_eq!(secrets, before);
    assert_eq!(request.headers[0].key, "Accept-Language");
}

#[test]
fn removing_a_secret_header_forgets_its_value_and_a_plain_one_does_not_touch_the_vault() {
    use ricochet_core::secrets::header_key;
    let mut request = secret_header_request();
    let mut secrets = std::collections::BTreeMap::new();
    set_secret_header(&mut request, &mut secrets, 1, "k-123");
    assert!(remove_header_and_secret(&mut request, &mut secrets, 0));
    assert!(secrets.contains_key(&header_key("r1", "X-Api-Key")));
    assert!(remove_header_and_secret(&mut request, &mut secrets, 0));
    assert!(secrets.is_empty());
    assert!(!remove_header_and_secret(&mut request, &mut secrets, 0));
}

#[test]
fn sending_fills_each_secret_header_from_the_vault_under_its_stored_name() {
    use ricochet_core::secrets::header_key;
    let stored = secret_header_request();
    let mut sent = stored.clone();
    sent.headers[1].key = "X-Api-Key".into();
    let mut vault = std::collections::BTreeMap::new();
    vault.insert(header_key("r1", "X-Api-Key"), "k-123".to_string());
    let missing = fill_secret_headers(&mut sent, &stored, |key| vault.get(key).cloned());
    assert!(!missing);
    assert_eq!(sent.headers[1].value, "k-123");
    assert_eq!(sent.headers[0].value, "text/plain", "plain headers are not touched");
}

#[test]
fn sending_reports_an_enabled_secret_header_with_nothing_stored_but_not_a_disabled_one() {
    let mut stored = secret_header_request();
    let mut sent = stored.clone();
    assert!(fill_secret_headers(&mut sent, &stored, |_| None));
    assert_eq!(sent.headers[1].value, "");
    stored.headers[1].enabled = false;
    assert!(!fill_secret_headers(&mut sent, &stored, |_| None));
}

#[test]
fn a_secret_header_value_is_never_written_by_set_field() {
    let mut request = secret_header_request();
    assert!(!set_field(&mut request, Field::HeaderValue(1), "hunter2"));
    assert_eq!(request.headers[1].value, "");
}

/// A typed header Secret is kept in the vault file, which holds no plain text, and never reaches the Workspace folder.
#[cfg(windows)]
#[test]
fn a_typed_secret_header_reaches_the_vault_encrypted_and_never_the_workspace_files() {
    use ricochet_core::secrets::{DpapiVault, SECRETS_FILE_NAME, SecretVault, header_key};

    const VALUE: &str = "hunter2-header-secret";
    let folder = temp_folder("secret-header-save");
    let personal = temp_folder("secret-header-personal");
    write_fixture(&folder);

    let (mut model, store) = ShellModel::open(Some(folder.clone()));
    let mut store = store.expect("the folder opened");
    let mut secrets = std::collections::BTreeMap::new();
    let request = find_request_mut(&mut model.workspace, "inner").unwrap();
    let request_id = request.id.clone();
    assert!(set_secret_header(request, &mut secrets, 1, VALUE));
    assert_eq!(store.save(&model.workspace).unwrap(), vec![]);

    // The Workspace files hold no copy of the value.
    fn read_all(dir: &std::path::Path, out: &mut Vec<u8>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                read_all(&path, out);
            } else {
                out.extend(std::fs::read(&path).unwrap());
            }
        }
    }
    let mut workspace_bytes = Vec::new();
    read_all(&folder, &mut workspace_bytes);
    assert!(!workspace_bytes.windows(VALUE.len()).any(|window| window == VALUE.as_bytes()));

    // The vault file holds the value only encrypted, and reads back to the same Secret.
    let vault = DpapiVault::new(personal.join(SECRETS_FILE_NAME));
    vault.save(&secrets).unwrap();
    let on_disk = std::fs::read(vault.path()).unwrap();
    assert!(!on_disk.windows(VALUE.len()).any(|window| window == VALUE.as_bytes()));
    let loaded = vault.load().unwrap();
    assert_eq!(loaded.get(&header_key(&request_id, "X-Key")).map(|value| value.expose()), Some(VALUE));

    std::fs::remove_dir_all(&folder).ok();
    std::fs::remove_dir_all(&personal).ok();
}
