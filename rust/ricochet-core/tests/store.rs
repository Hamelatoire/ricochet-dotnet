//! Tests for the workspace store, ported from `WorkspaceFolderTests` and the `WorkspaceStoreTests` they sit beside.
//! Each test works in its own temporary folder, which is removed afterwards.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime};

use pretty_assertions::assert_eq;
use ricochet_core::models::*;
use ricochet_core::store::*;
use serde_json::{Value, json};

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("ricochet-{tag}-{}", new_id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn request(name: &str, id: &str) -> ApiRequest {
    ApiRequest { id: id.to_string(), name: name.to_string(), url: format!("http://x.test/{name}"), ..ApiRequest::default() }
}

fn request_item(name: &str, id: &str) -> CollectionItem {
    CollectionItem { id: id.to_string(), name: name.to_string(), request: Some(request(name, &format!("r{id}"))), ..CollectionItem::default() }
}

fn sample() -> Workspace {
    let orders = CollectionItem {
        id: "f1aaaaaaaa".into(),
        name: "Orders".into(),
        scripts: vec![StoredScript { listen: "prerequest".into(), source: "pm.b();".into() }],
        children: vec![request_item("Create", "i2aaaaaaaa")],
        ..CollectionItem::default()
    };
    Workspace {
        collections: vec![Collection {
            id: "c1aaaaaaaa".into(),
            name: "Shop API".into(),
            variables: vec![KeyValue::new("base", "http://x.test")],
            scripts: vec![StoredScript { listen: "test".into(), source: "pm.a();".into() }],
            items: vec![request_item("Zebra", "i1aaaaaaaa"), orders, request_item("Apple", "i3aaaaaaaa")],
            ..Collection::default()
        }],
        environments: vec![ApiEnvironment { id: "e1aaaaaaaa".into(), name: "Staging".into(), variables: vec![KeyValue::new("a", "1")] }],
        globals: vec![KeyValue::new("g", "1")],
        ..Workspace::default()
    }
}

fn load(root: &Path) -> (Workspace, Vec<WorkspaceIssue>) {
    WorkspaceFolder::new(root).load().unwrap()
}

fn saved(root: &Path) -> WorkspaceFolder {
    let mut folder = WorkspaceFolder::new(root);
    folder.load().unwrap();
    folder.save(&sample()).unwrap();
    folder
}

fn request_path(root: &Path, name: &str, id: &str) -> PathBuf {
    root.join("collections").join("shop-api-c1aaaaaa").join(format!("{name}-{id}.json"))
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(files_under(&path));
            } else {
                found.push(path);
            }
        }
    }
    found
}

fn modified_times(dir: &Path) -> BTreeMap<PathBuf, SystemTime> {
    files_under(dir).into_iter().map(|path| {
        let time = fs::metadata(&path).unwrap().modified().unwrap();
        (path, time)
    }).collect()
}

fn json_at(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn names(items: &[CollectionItem]) -> Vec<&str> {
    items.iter().map(|item| item.name.as_str()).collect()
}

// --- WorkspaceFolder ---

#[test]
fn round_trips_everything_in_order() {
    let dir = TempDir::new("round-trip");
    saved(dir.path());

    let (loaded, issues) = load(dir.path());

    assert!(issues.is_empty());
    assert_eq!(loaded.collections.len(), 1);
    let collection = &loaded.collections[0];
    assert_eq!(collection.name, "Shop API");
    assert_eq!(names(&collection.items), ["Zebra", "Orders", "Apple"]);
    assert_eq!(collection.items[1].children[0].name, "Create");
    assert_eq!(collection.items[1].children[0].request.as_ref().unwrap().id, "ri2aaaaaaaa");
    assert_eq!(collection.scripts[0].source, "pm.a();");
    assert_eq!(collection.items[1].scripts[0].source, "pm.b();");
    assert_eq!(loaded.environments[0].variables[0].value, "1");
    assert_eq!(loaded.globals[0].value, "1");
}

#[test]
fn collections_keep_the_order_they_were_saved_in() {
    let dir = TempDir::new("order");
    let mut workspace = sample();
    let second = Collection { id: "c2aaaaaaaa".into(), name: "Auth API".into(), ..Collection::default() };
    let third = Collection { id: "c3aaaaaaaa".into(), name: "Billing".into(), ..Collection::default() };
    workspace.collections = vec![third, workspace.collections[0].clone(), second];
    let mut folder = WorkspaceFolder::new(dir.path());
    folder.load().unwrap();
    folder.save(&workspace).unwrap();

    let (loaded, issues) = load(dir.path());

    assert!(issues.is_empty());
    let loaded_names: Vec<&str> = loaded.collections.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(loaded_names, ["Billing", "Shop API", "Auth API"]);
}

#[test]
fn uses_one_readable_file_per_request() {
    let dir = TempDir::new("layout");
    saved(dir.path());
    let root = dir.path();

    assert!(request_path(root, "zebra", "i1aaaaaa").is_file());
    assert!(root.join("collections/shop-api-c1aaaaaa/orders-f1aaaaaa/create-i2aaaaaa.json").is_file());
    assert!(root.join("environments/staging-e1aaaaaa.json").is_file());
    assert!(root.join("globals.json").is_file());
    assert!(root.join(MARKER_FILE_NAME).is_file());

    let text = fs::read_to_string(request_path(root, "zebra", "i1aaaaaa")).unwrap();
    assert!(!text.contains("\r\n"));
    assert!(text.ends_with("}\n"));
}

#[test]
fn id_stays_the_same_across_loads() {
    let dir = TempDir::new("id");
    let mut first = saved(dir.path());
    let id = first.id().to_string();
    assert!(!id.is_empty());

    let mut again = WorkspaceFolder::new(dir.path());
    again.load().unwrap();
    assert_eq!(again.id(), id);
    first.load().unwrap();
}

#[test]
fn saving_writes_only_what_changed() {
    let dir = TempDir::new("only-changed");
    let mut folder = saved(dir.path());
    let before = modified_times(dir.path());
    thread::sleep(Duration::from_millis(50));

    let mut changed = sample();
    let zebra = &mut changed.collections[0].items[0];
    zebra.request = Some(ApiRequest { url: "http://changed.test".into(), ..request("Zebra", "ri1aaaaaaaa") });
    folder.save(&changed).unwrap();

    let after = modified_times(dir.path());
    let written: Vec<String> = after
        .iter()
        .filter(|(path, time)| before.get(*path) != Some(*time))
        .map(|(path, _)| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(written, ["zebra-i1aaaaaa.json"]);
}

#[test]
fn a_fresh_load_then_save_writes_nothing() {
    let dir = TempDir::new("no-op");
    saved(dir.path());
    let before = modified_times(dir.path());
    thread::sleep(Duration::from_millis(50));

    let mut folder = WorkspaceFolder::new(dir.path());
    let (loaded, _) = folder.load().unwrap();
    folder.save(&loaded).unwrap();

    assert_eq!(before, modified_times(dir.path()));
}

#[test]
fn windows_line_endings_from_git_do_not_cause_rewrites() {
    let dir = TempDir::new("crlf");
    saved(dir.path());
    let path = request_path(dir.path(), "zebra", "i1aaaaaa");
    fs::write(&path, fs::read_to_string(&path).unwrap().replace('\n', "\r\n")).unwrap();
    let before = fs::metadata(&path).unwrap().modified().unwrap();
    thread::sleep(Duration::from_millis(50));

    let mut folder = WorkspaceFolder::new(dir.path());
    let (loaded, _) = folder.load().unwrap();
    folder.save(&loaded).unwrap();

    assert_eq!(before, fs::metadata(&path).unwrap().modified().unwrap());
    assert!(!folder.has_outside_changes());
}

#[test]
fn deleting_and_renaming_remove_old_files_and_empty_folders() {
    let dir = TempDir::new("delete");
    let mut folder = saved(dir.path());
    let sample = sample();
    let mut renamed = sample.collections[0].clone();
    renamed.name = "Renamed".into();
    renamed.items = vec![sample.collections[0].items[0].clone()];
    folder.save(&Workspace { collections: vec![renamed], ..Workspace::default() }).unwrap();

    let root = dir.path();
    assert!(!root.join("collections/shop-api-c1aaaaaa").exists());
    assert!(root.join("collections/renamed-c1aaaaaa/zebra-i1aaaaaa.json").is_file());
    assert!(!root.join("environments").exists());
}

#[test]
fn same_names_get_separate_files() {
    let dir = TempDir::new("same-name");
    let workspace = Workspace {
        collections: vec![Collection {
            id: "c1aaaaaaaa".into(),
            name: "Shop API".into(),
            items: vec![request_item("Same", "i1aaaaaaaa"), request_item("Same", "i2aaaaaaaa")],
            ..Collection::default()
        }],
        ..Workspace::default()
    };
    let mut folder = WorkspaceFolder::new(dir.path());
    folder.load().unwrap();
    folder.save(&workspace).unwrap();

    let (loaded, issues) = load(dir.path());
    assert!(issues.is_empty());
    assert_eq!(loaded.collections[0].items.len(), 2);
}

#[test]
fn ids_sharing_a_prefix_do_not_overwrite_each_other() {
    let dir = TempDir::new("prefix");
    let workspace = Workspace {
        collections: vec![Collection {
            id: "c1aaaaaaaa".into(),
            name: "Shop API".into(),
            items: vec![request_item("Same", "aaaaaaaa11"), request_item("Same", "aaaaaaaa22")],
            ..Collection::default()
        }],
        ..Workspace::default()
    };
    let mut folder = WorkspaceFolder::new(dir.path());
    folder.load().unwrap();
    folder.save(&workspace).unwrap();

    let (loaded, _) = load(dir.path());
    let mut ids: Vec<&str> = loaded.collections[0].items.iter().map(|i| i.id.as_str()).collect();
    ids.sort();
    assert_eq!(ids, ["aaaaaaaa11", "aaaaaaaa22"]);
}

#[test]
fn slugs_are_safe_for_every_file_system() {
    assert_eq!(slug("Orders / Create #1!", "abcdef1234567890"), "orders-create-1-abcdef12");
    assert_eq!(slug("***", "abcdef1234567890"), "item-abcdef12");
    assert_eq!(slug("Écran", "abcdef1234567890"), "cran-abcdef12");
}

#[test]
fn merge_conflict_is_reported_and_the_file_survives_saving() {
    let dir = TempDir::new("conflict");
    let mut folder = saved(dir.path());
    let path = request_path(dir.path(), "zebra", "i1aaaaaa");
    let conflicted = "<<<<<<< HEAD\n{ }\n=======\n{ }\n>>>>>>> theirs\n";
    fs::write(&path, conflicted).unwrap();

    let (loaded, issues) = folder.load().unwrap();

    assert!(issues.iter().any(|i| i.path.ends_with("zebra-i1aaaaaa.json")));
    assert!(!loaded.collections[0].items.iter().any(|i| i.name == "Zebra"));
    assert_eq!(names(&loaded.collections[0].items), ["Orders", "Apple"]);

    folder.save(&loaded).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), conflicted);
}

#[test]
fn conflict_in_a_collection_file_keeps_all_of_its_children() {
    let dir = TempDir::new("collection-conflict");
    let mut folder = saved(dir.path());
    let collection_file = dir.path().join("collections/shop-api-c1aaaaaa/collection.json");
    fs::write(&collection_file, "<<<<<<< broken").unwrap();

    let (loaded, issues) = folder.load().unwrap();
    assert!(loaded.collections.is_empty());
    assert!(!issues.is_empty());

    folder.save(&loaded).unwrap();

    assert!(request_path(dir.path(), "zebra", "i1aaaaaa").is_file());
    assert_eq!(fs::read_to_string(&collection_file).unwrap(), "<<<<<<< broken");
}

#[test]
fn item_a_merge_added_but_did_not_list_still_loads() {
    let dir = TempDir::new("merge-added");
    saved(dir.path());
    let extra = request_path(dir.path(), "new-one", "i9aaaaaa");
    fs::write(&extra, r#"{ "id": "i9aaaaaaaa", "name": "New One", "request": { "id": "x", "name": "New One", "url": "u" } }"#).unwrap();

    let (loaded, issues) = load(dir.path());

    assert!(issues.is_empty());
    assert_eq!(names(&loaded.collections[0].items), ["Zebra", "Orders", "Apple", "New One"]);
}

#[test]
fn order_naming_a_missing_file_is_reported() {
    let dir = TempDir::new("missing");
    saved(dir.path());
    fs::remove_file(request_path(dir.path(), "zebra", "i1aaaaaa")).unwrap();

    let (loaded, issues) = load(dir.path());

    assert_eq!(names(&loaded.collections[0].items), ["Orders", "Apple"]);
    assert!(issues.iter().any(|i| i.problem.contains("i1aaaaaaaa")));
}

#[test]
fn copied_file_with_a_duplicate_id_is_reported_and_kept() {
    let dir = TempDir::new("duplicate");
    let mut folder = saved(dir.path());
    let copy = request_path(dir.path(), "zebra-copy", "i1aaaaaa");
    fs::copy(request_path(dir.path(), "zebra", "i1aaaaaa"), &copy).unwrap();

    let (loaded, issues) = folder.load().unwrap();
    assert!(issues.iter().any(|i| i.problem.contains("already uses")));

    folder.save(&loaded).unwrap();
    assert!(copy.is_file());
    assert!(request_path(dir.path(), "zebra", "i1aaaaaa").is_file());
}

#[test]
fn a_duplicate_with_different_content_is_never_overwritten() {
    let dir = TempDir::new("duplicate-edited");
    let mut folder = saved(dir.path());
    let original = request_path(dir.path(), "zebra", "i1aaaaaa");
    let original_text = fs::read_to_string(&original).unwrap();
    let copy = request_path(dir.path(), "zebra-copy", "i1aaaaaa");
    let copy_text = original_text.replace("http://x.test/Zebra", "http://edited.test");
    fs::write(&copy, &copy_text).unwrap();

    let (loaded, _) = folder.load().unwrap();
    folder.save(&loaded).unwrap();

    assert_eq!(fs::read_to_string(&original).unwrap(), original_text);
    assert_eq!(fs::read_to_string(&copy).unwrap(), copy_text);
}

#[test]
fn a_file_renamed_by_hand_keeps_its_name_until_the_item_is_renamed() {
    let dir = TempDir::new("hand-rename");
    let mut folder = saved(dir.path());
    let renamed = request_path(dir.path(), "my-own-name", "x");
    fs::rename(request_path(dir.path(), "zebra", "i1aaaaaa"), &renamed).unwrap();

    let (mut loaded, _) = folder.load().unwrap();
    folder.save(&loaded).unwrap();
    assert!(renamed.is_file());
    assert!(!request_path(dir.path(), "zebra", "i1aaaaaa").exists());

    loaded.collections[0].items[0].name = "Giraffe".into();
    folder.save(&loaded).unwrap();
    assert!(!renamed.exists());
    assert!(request_path(dir.path(), "giraffe", "i1aaaaaa").is_file());
}

#[test]
fn a_new_item_never_writes_over_an_unreadable_file_at_its_path() {
    let dir = TempDir::new("blocked");
    let mut folder = saved(dir.path());
    let blocked = request_path(dir.path(), "apple", "i3aaaaaa");
    fs::write(&blocked, "<<<<<<< conflict").unwrap();

    let (mut loaded, _) = folder.load().unwrap();
    let restored = CollectionItem { id: "i3aaaaaaaa".into(), name: "Apple".into(), request: Some(request("Apple", "ri3")), ..CollectionItem::default() };
    loaded.collections[0].items.push(restored);
    folder.save(&loaded).unwrap();

    assert_eq!(fs::read_to_string(&blocked).unwrap(), "<<<<<<< conflict");
    assert!(request_path(dir.path(), "apple-i3aaaaaa", "i3aaaaaaaa").is_file());
}

#[test]
fn unknown_files_are_left_alone() {
    let dir = TempDir::new("unknown");
    let mut folder = saved(dir.path());
    let readme = dir.path().join("README.md");
    let git_config = dir.path().join(".git/config");
    fs::create_dir_all(git_config.parent().unwrap()).unwrap();
    fs::write(&readme, "hi").unwrap();
    fs::write(&git_config, "x").unwrap();

    folder.save(&Workspace::default()).unwrap();

    assert!(readme.is_file());
    assert!(git_config.is_file());
    assert!(!folder.has_outside_changes());
}

#[test]
fn explicit_nulls_in_a_hand_edited_file_do_not_crash() {
    let dir = TempDir::new("nulls");
    saved(dir.path());
    fs::write(
        request_path(dir.path(), "zebra", "i1aaaaaa"),
        r#"{ "id": "i1aaaaaaaa", "name": null, "scripts": null, "request": { "id": null, "name": null, "url": null, "method": null, "headers": null, "body": null } }"#,
    )
    .unwrap();

    let (loaded, _) = load(dir.path());

    let item = loaded.collections[0].items.iter().find(|i| i.id == "i1aaaaaaaa").unwrap();
    let request = item.request.as_ref().unwrap();
    assert_eq!(request.method, "GET");
    assert!(request.headers.is_empty());
    assert_eq!(request.url, "");
}

#[test]
fn own_saves_are_not_outside_changes_but_other_edits_are() {
    let dir = TempDir::new("outside");
    let mut folder = saved(dir.path());
    assert!(!folder.has_outside_changes());

    let mut changed = sample();
    changed.globals = vec![KeyValue::new("g", "2")];
    folder.save(&changed).unwrap();
    assert!(!folder.has_outside_changes());

    fs::write(dir.path().join("globals.json"), r#"[ { "key": "g", "value": "3" } ]"#).unwrap();
    assert!(folder.has_outside_changes());

    let (loaded, _) = folder.load().unwrap();
    assert_eq!(loaded.globals[0].value, "3");
    assert!(!folder.has_outside_changes());
}

#[test]
fn added_and_removed_files_are_outside_changes() {
    let dir = TempDir::new("added-removed");
    let folder = saved(dir.path());
    let path = request_path(dir.path(), "zebra", "i1aaaaaa");
    let text = fs::read_to_string(&path).unwrap();

    fs::remove_file(&path).unwrap();
    assert!(folder.has_outside_changes());

    fs::write(&path, text).unwrap();
    assert!(!folder.has_outside_changes());
}

#[test]
fn hidden_item_ids_are_saved_and_loaded_unchanged() {
    let dir = TempDir::new("hidden");
    let mut workspace = sample();
    let hidden = vec!["i3aaaaaaaa".to_string(), "f1aaaaaaaa".to_string()];
    workspace.collections[0].hidden_item_ids = Some(hidden.clone());
    let mut folder = WorkspaceFolder::new(dir.path());
    folder.load().unwrap();
    folder.save(&workspace).unwrap();

    let (loaded, issues) = load(dir.path());

    assert!(issues.is_empty());
    assert_eq!(loaded.collections[0].hidden_item_ids, Some(hidden));
}

#[test]
fn a_collection_with_no_hidden_items_writes_no_hidden_key() {
    let dir = TempDir::new("hidden-key");
    let mut folder = saved(dir.path());
    let collection_file = dir.path().join("collections/shop-api-c1aaaaaa/collection.json");
    assert!(json_at(&collection_file).get("hiddenItemIds").is_none());

    let mut workspace = sample();
    workspace.collections[0].hidden_item_ids = Some(vec!["i3aaaaaaaa".into()]);
    folder.save(&workspace).unwrap();
    assert!(json_at(&collection_file).get("hiddenItemIds").is_some());

    workspace.collections[0].hidden_item_ids = None;
    folder.save(&workspace).unwrap();
    assert!(json_at(&collection_file).get("hiddenItemIds").is_none());
    assert_eq!(load(dir.path()).0.collections[0].hidden_item_ids, None);
}

#[test]
fn folder_tags_are_saved_loaded_and_normalized() {
    let dir = TempDir::new("tags");
    let mut workspace = sample();
    workspace.collections[0].items[1].tags = Some(vec!["  smoke ".into(), "SMOKE".into(), "".into(), "Slow".into()]);
    let mut folder = WorkspaceFolder::new(dir.path());
    folder.load().unwrap();
    folder.save(&workspace).unwrap();

    let (loaded, issues) = load(dir.path());

    assert!(issues.is_empty());
    let orders = loaded.collections[0].items.iter().find(|i| i.id == "f1aaaaaaaa").unwrap();
    assert_eq!(orders.tags, Some(vec!["smoke".to_string(), "Slow".to_string()]));
}

#[test]
fn a_folder_with_no_tags_writes_no_tags_key() {
    let dir = TempDir::new("no-tags");
    saved(dir.path());
    let folder_file = dir.path().join("collections/shop-api-c1aaaaaa/orders-f1aaaaaa/folder.json");
    assert!(json_at(&folder_file).get("tags").is_none());
}

#[test]
fn a_request_moved_to_another_folder_on_disk_loads_under_its_new_parent() {
    let dir = TempDir::new("moved");
    let mut folder = saved(dir.path());
    let to_dir = dir.path().join("collections/shop-api-c1aaaaaa/orders-f1aaaaaa");
    fs::rename(request_path(dir.path(), "zebra", "i1aaaaaa"), to_dir.join("zebra-i1aaaaaa.json")).unwrap();

    let (loaded, _) = folder.load().unwrap();

    let orders = loaded.collections[0].items.iter().find(|i| i.id == "f1aaaaaaaa").unwrap();
    assert!(orders.children.iter().any(|c| c.id == "i1aaaaaaaa" && c.name == "Zebra"));
    assert!(!loaded.collections[0].items.iter().any(|i| i.id == "i1aaaaaaaa"));

    // Saving keeps the moved Request where it now is, and the next load has nothing to report.
    folder.save(&loaded).unwrap();
    let (reloaded, issues) = load(dir.path());
    assert!(issues.is_empty());
    let orders = reloaded.collections[0].items.iter().find(|i| i.id == "f1aaaaaaaa").unwrap();
    let child_names: Vec<&str> = orders.children.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(child_names, ["Create", "Zebra"]);
    assert!(to_dir.join("zebra-i1aaaaaa.json").is_file());
}

#[test]
fn a_conflicted_environment_is_reported_and_kept_while_everything_else_loads() {
    let dir = TempDir::new("env-conflict");
    let mut folder = saved(dir.path());
    let environment_file = dir.path().join("environments/staging-e1aaaaaa.json");
    let conflicted = "<<<<<<< HEAD\n{ }\n=======\n{ }\n>>>>>>> theirs\n";
    fs::write(&environment_file, conflicted).unwrap();

    let (loaded, issues) = folder.load().unwrap();

    assert!(issues.iter().any(|i| i.path == "environments/staging-e1aaaaaa.json"));
    assert!(loaded.environments.is_empty());
    assert_eq!(names(&loaded.collections[0].items), ["Zebra", "Orders", "Apple"]);
    assert_eq!(loaded.globals[0].value, "1");

    folder.save(&loaded).unwrap();
    assert_eq!(fs::read_to_string(&environment_file).unwrap(), conflicted);
}

#[test]
fn a_gitignore_the_user_edited_is_kept_on_the_next_save() {
    let dir = TempDir::new("gitignore-edit");
    let mut folder = saved(dir.path());
    let gitignore = dir.path().join(GITIGNORE_FILE_NAME);
    fs::write(&gitignore, format!("{}scratch/\n", fs::read_to_string(&gitignore).unwrap())).unwrap();

    let mut changed = sample();
    changed.globals = vec![KeyValue::new("g", "2")];
    folder.save(&changed).unwrap();
    assert!(fs::read_to_string(&gitignore).unwrap().ends_with("scratch/\n"));
    assert!(!folder.has_outside_changes());

    let mut fresh = WorkspaceFolder::new(dir.path());
    let (loaded, _) = fresh.load().unwrap();
    fresh.save(&loaded).unwrap();
    assert!(fs::read_to_string(&gitignore).unwrap().ends_with("scratch/\n"));
}

// --- WorkspaceStore ---

fn store_sample(active: &str) -> Workspace {
    Workspace {
        collections: vec![Collection {
            id: "c1aaaaaaaa".into(),
            name: "C".into(),
            items: vec![CollectionItem { id: "i1aaaaaaaa".into(), name: "R".into(), request: Some(request("R", "ri1")), ..CollectionItem::default() }],
            ..Collection::default()
        }],
        environments: vec![ApiEnvironment { id: "e1aaaaaaaa".into(), name: "E".into(), variables: Vec::new() }],
        active_environment_id: Some(active.into()),
        globals: vec![KeyValue::new("g", "1")],
        history: vec![HistoryEntry {
            request: ApiRequest { url: "h".into(), ..ApiRequest::default() },
            status_code: Some(200),
            elapsed_ms: 1,
            at: chrono::DateTime::parse_from_rfc3339("1970-01-01T00:00:00+00:00").unwrap(),
            collection_id: Some("c1aaaaaaaa".into()),
            certificates_unverified: None,
        }],
        ..Workspace::default()
    }
}

fn opened(dir: &Path) -> WorkspaceStore {
    let mut store = WorkspaceStore::new(dir);
    store.load().unwrap();
    store
}

#[test]
fn fresh_start_is_empty() {
    let dir = TempDir::new("fresh");
    let result = WorkspaceStore::new(dir.path()).load().unwrap();
    assert!(result.workspace.collections.is_empty());
    assert!(result.issues.is_empty());
}

#[test]
fn round_trips_and_keeps_personal_data_out_of_the_shared_folder() {
    let dir = TempDir::new("store-round-trip");
    let mut store = opened(dir.path());
    store.save(&store_sample("e1aaaaaaaa")).unwrap();

    let loaded = WorkspaceStore::new(dir.path()).load().unwrap().workspace;
    assert_eq!(loaded.collections[0].name, "C");
    assert_eq!(loaded.active_environment_id.as_deref(), Some("e1aaaaaaaa"));
    assert_eq!(loaded.history[0].collection_id.as_deref(), Some("c1aaaaaaaa"));

    assert!(dir.path().join("history.json").is_file());
    assert!(dir.path().join("state.json").is_file());
    let shared: Vec<String> = files_under(store.folder_path().unwrap()).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert!(!shared.iter().any(|name| name == "history.json" || name == "state.json" || name == "secrets.dat"));
}

#[test]
fn state_keeps_fields_this_port_does_not_read() {
    let dir = TempDir::new("state-keeps");
    fs::write(dir.path().join("state.json"), r#"{ "theme": "light", "settings": { "requestTimeoutSeconds": 30 } }"#).unwrap();

    let mut store = opened(dir.path());
    store.save(&store_sample("e1aaaaaaaa")).unwrap();

    let state = json_at(&dir.path().join("state.json"));
    assert_eq!(state["theme"], json!("light"));
    assert_eq!(state["settings"]["requestTimeoutSeconds"], json!(30));
    assert_eq!(state["activeEnvironmentId"], json!("e1aaaaaaaa"));
}

#[test]
fn a_renamed_folder_saves_under_its_new_name_and_keeps_its_requests() {
    let dir = TempDir::new("store-rename");
    let folder = CollectionItem { id: "f1".into(), name: "Old name".into(), children: vec![request_item("Get", "i1")], ..CollectionItem::default() };
    let collection = Collection { id: "c1".into(), name: "C".into(), items: vec![folder.clone()], ..Collection::default() };
    let mut store = opened(dir.path());
    store.save(&Workspace { collections: vec![collection.clone()], ..Workspace::default() }).unwrap();

    let renamed = Collection { items: vec![CollectionItem { name: "New name".into(), ..folder }], ..collection };
    store.save(&Workspace { collections: vec![renamed], ..Workspace::default() }).unwrap();

    let reloaded = WorkspaceStore::new(dir.path()).load().unwrap().workspace;
    let item = &reloaded.collections[0].items;
    assert_eq!(item.len(), 1);
    assert_eq!(item[0].name, "New name");
    assert_eq!(item[0].children.len(), 1);
    assert_eq!(item[0].children[0].name, "Get");
}

#[test]
fn a_new_workspace_gets_a_gitignore_that_excludes_personal_files() {
    let dir = TempDir::new("gitignore-new");
    let mut store = opened(dir.path());
    store.save(&store_sample("e1aaaaaaaa")).unwrap();

    let text = fs::read_to_string(store.folder_path().unwrap().join(GITIGNORE_FILE_NAME)).unwrap();
    assert!(text.contains("state.json"));
    assert!(text.contains("secrets.dat"));
    assert!(text.contains("*.tmp"));
}

#[test]
fn an_existing_gitignore_is_never_overwritten() {
    let dir = TempDir::new("gitignore-existing");
    let mut store = opened(dir.path());
    let path = store.folder_path().unwrap().join(GITIGNORE_FILE_NAME);
    fs::write(&path, "my-own-rule\n").unwrap();

    store.save(&store_sample("e1aaaaaaaa")).unwrap();

    assert_eq!(fs::read_to_string(&path).unwrap(), "my-own-rule\n");
}

#[test]
fn a_gitignore_the_user_deleted_is_not_recreated() {
    let dir = TempDir::new("gitignore-deleted");
    let mut store = opened(dir.path());
    store.save(&store_sample("e1aaaaaaaa")).unwrap();
    let path = store.folder_path().unwrap().join(GITIGNORE_FILE_NAME);
    fs::remove_file(&path).unwrap();

    store.save(&store_sample("e1aaaaaaaa")).unwrap();

    assert!(!path.exists());
}

#[test]
fn original_single_file_is_migrated_then_kept_as_a_backup() {
    let dir = TempDir::new("legacy");
    let legacy = dir.path().join("workspace.json");
    fs::write(
        &legacy,
        r#"{
              "collections": [ { "id": "c1", "name": "Old", "items": [ { "id": "i1", "name": "R", "request": { "id": "r1", "name": "R", "url": "u" } } ] } ],
              "environments": [ { "id": "e1", "name": "E" } ], "activeEnvironmentId": "e1",
              "globals": [ { "key": "a", "value": "1", "enabled": true } ],
              "history": [ { "request": { "id": "h", "url": "u" }, "statusCode": 200, "elapsedMs": 1, "at": "2026-11-04T00:00:00+00:00" } ]
            }"#,
    )
    .unwrap();

    let mut store = WorkspaceStore::new(dir.path());
    let first = store.load().unwrap();
    assert!(first.needs_save);
    assert_eq!(first.workspace.collections[0].name, "Old");
    assert!(legacy.is_file()); // Nothing moves until the new files are safely written.

    store.save(&first.workspace).unwrap();
    assert!(!legacy.exists());
    assert!(dir.path().join("workspace.json.bak").is_file());

    let second = WorkspaceStore::new(dir.path()).load().unwrap();
    assert!(!second.needs_save);
    assert_eq!(second.workspace.collections[0].name, "Old");
    assert_eq!(second.workspace.active_environment_id.as_deref(), Some("e1"));
    assert_eq!(second.workspace.history.len(), 1);
}

#[test]
fn migration_handles_the_very_first_file_format() {
    let dir = TempDir::new("legacy-first");
    fs::write(
        dir.path().join("workspace.json"),
        r#"{ "saved": [ { "id": "r1", "name": "Ping", "url": "http://x.test" } ], "variables": [ { "key": "a", "value": "1" } ] }"#,
    )
    .unwrap();

    let mut store = WorkspaceStore::new(dir.path());
    let loaded = store.load().unwrap().workspace;
    store.save(&loaded).unwrap();

    let again = WorkspaceStore::new(dir.path()).load().unwrap().workspace;
    assert_eq!(again.collections[0].name, DEFAULT_COLLECTION_NAME);
    assert_eq!(again.collections[0].items[0].request.as_ref().unwrap().id, "r1");
    assert_eq!(again.globals[0].key, "a");
}

#[test]
fn corrupt_personal_files_are_set_aside_and_reported() {
    let dir = TempDir::new("corrupt");
    fs::write(dir.path().join("history.json"), "{broken").unwrap();

    let result = WorkspaceStore::new(dir.path()).load().unwrap();

    assert!(result.issues.iter().any(|i| i.path.contains(".corrupt-")));
    let kept: Vec<PathBuf> = files_under(dir.path()).into_iter().filter(|p| p.to_string_lossy().contains(".corrupt-")).collect();
    assert_eq!(kept.len(), 1);
    assert_eq!(fs::read_to_string(&kept[0]).unwrap(), "{broken");
}

#[test]
fn missing_workspace_folder_is_reported_and_not_recreated() {
    let dir = TempDir::new("unplugged");
    let gone = dir.path().join("unplugged");
    fs::write(dir.path().join("state.json"), json!({ "workspaceFolder": gone.to_string_lossy() }).to_string()).unwrap();

    let mut store = WorkspaceStore::new(dir.path());
    let result = store.load().unwrap();

    assert!(result.issues.iter().any(|i| Path::new(&i.path) == gone));
    assert!(!gone.exists());
    assert_ne!(store.folder_path().unwrap(), gone);
}

#[test]
fn hidden_items_are_kept_across_restarts() {
    let dir = TempDir::new("store-hidden");
    let mut store = opened(dir.path());
    let mut workspace = store_sample("e1aaaaaaaa");
    workspace.collections[0].hidden_item_ids = Some(vec!["i1aaaaaaaa".into()]);
    store.save(&workspace).unwrap();

    let reloaded = WorkspaceStore::new(dir.path()).load().unwrap().workspace;

    assert_eq!(reloaded.collections[0].hidden_item_ids, Some(vec!["i1aaaaaaaa".to_string()]));
}

#[test]
fn a_store_saves_only_after_it_has_loaded() {
    let dir = TempDir::new("not-loaded");
    let mut store = WorkspaceStore::new(dir.path());
    assert!(store.save(&store_sample("e1aaaaaaaa")).is_err());
    assert_eq!(store.folder_path(), None);
}
