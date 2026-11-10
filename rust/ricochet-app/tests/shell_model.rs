//! Tests for the shell's view-model logic: the sidebar tree, the status bar and the folder the shell opens.
//! No window is needed. Folders are throwaway temp directories, never a real Workspace.

use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use pretty_assertions::assert_eq;
use ricochet_app::shell::{ShellModel, folder_from};
use ricochet_app::status::{NO_ENVIRONMENT, environment_name, status_bar};
use ricochet_app::tree::{RowKind, build_tree, find_request};
use ricochet_core::models::{ApiEnvironment, ApiRequest, Collection, CollectionItem, Workspace};
use ricochet_core::store::WorkspaceFolder;

fn request(id: &str, name: &str, method: &str) -> ApiRequest {
    ApiRequest { id: id.into(), name: name.into(), method: method.into(), url: format!("https://example.test/{id}"), ..Default::default() }
}

fn request_item(id: &str, name: &str, method: &str) -> CollectionItem {
    CollectionItem { id: id.into(), name: name.into(), request: Some(request(id, name, method)), ..Default::default() }
}

fn folder_item(id: &str, name: &str, children: Vec<CollectionItem>) -> CollectionItem {
    CollectionItem { id: id.into(), name: name.into(), children, ..Default::default() }
}

/// Two Collections: "Shop" has a Folder with two Requests and a loose Request; "Health" has one Request.
fn sample_workspace() -> Workspace {
    let shop = Collection {
        id: "shop".into(),
        name: "Shop".into(),
        items: vec![
            folder_item("orders", "Orders", vec![request_item("list-orders", "List orders", "GET"), request_item("place-order", "Place order", "POST")]),
            request_item("ping", "Ping", "HEAD"),
        ],
        ..Default::default()
    };
    let health = Collection {
        id: "health".into(),
        name: "Health".into(),
        items: vec![request_item("status", "Status", "GET")],
        ..Default::default()
    };
    Workspace { collections: vec![shop, health], ..Default::default() }
}

#[test]
fn tree_lists_collections_then_their_items_depth_first() {
    let rows = build_tree(&sample_workspace(), false);
    let shape: Vec<(&str, usize, RowKind)> = rows.iter().map(|row| (row.id.as_str(), row.depth, row.kind)).collect();
    assert_eq!(
        shape,
        vec![
            ("shop", 0, RowKind::Collection),
            ("orders", 1, RowKind::Folder),
            ("list-orders", 2, RowKind::Request),
            ("place-order", 2, RowKind::Request),
            ("ping", 1, RowKind::Request),
            ("health", 0, RowKind::Collection),
            ("status", 1, RowKind::Request),
        ]
    );
}

#[test]
fn requests_carry_their_method_and_folders_and_collections_carry_none() {
    let rows = build_tree(&sample_workspace(), false);
    let methods: Vec<(&str, Option<&str>)> = rows.iter().map(|row| (row.label.as_str(), row.method.as_deref())).collect();
    assert_eq!(
        methods,
        vec![
            ("Shop", None),
            ("Orders", None),
            ("List orders", Some("GET")),
            ("Place order", Some("POST")),
            ("Ping", Some("HEAD")),
            ("Health", None),
            ("Status", Some("GET")),
        ]
    );
}

#[test]
fn hidden_items_and_everything_inside_them_are_left_out_unless_asked_for() {
    let mut workspace = sample_workspace();
    workspace.collections[0].hidden_item_ids = Some(vec!["orders".into(), "ping".into()]);

    let shown: Vec<String> = build_tree(&workspace, false).into_iter().map(|row| row.id).collect();
    assert_eq!(shown, vec!["shop", "health", "status"]);

    let all = build_tree(&workspace, true);
    let hidden: Vec<(&str, bool)> = all.iter().map(|row| (row.id.as_str(), row.hidden)).collect();
    assert_eq!(
        hidden,
        vec![
            ("shop", false),
            ("orders", true),
            ("list-orders", true),
            ("place-order", true),
            ("ping", true),
            ("health", false),
            ("status", false),
        ]
    );
}

#[test]
fn an_empty_workspace_has_no_rows() {
    assert!(build_tree(&Workspace::default(), false).is_empty());
}

#[test]
fn find_request_reaches_requests_inside_folders_and_ignores_folders() {
    let workspace = sample_workspace();
    assert_eq!(find_request(&workspace, "place-order").map(|r| r.method.as_str()), Some("POST"));
    assert_eq!(find_request(&workspace, "status").map(|r| r.name.as_str()), Some("Status"));
    assert_eq!(find_request(&workspace, "orders"), None);
    assert_eq!(find_request(&workspace, "missing"), None);
}

#[test]
fn environment_name_is_the_active_one_and_falls_back_when_missing() {
    let mut workspace = sample_workspace();
    workspace.environments = vec![
        ApiEnvironment { id: "dev".into(), name: "Dev".into(), ..Default::default() },
        ApiEnvironment { id: "staging".into(), name: "Staging".into(), ..Default::default() },
    ];
    assert_eq!(environment_name(&workspace), NO_ENVIRONMENT);

    workspace.active_environment_id = Some("staging".into());
    assert_eq!(environment_name(&workspace), "Staging");

    workspace.active_environment_id = Some("deleted".into());
    assert_eq!(environment_name(&workspace), NO_ENVIRONMENT);
}

#[test]
fn status_bar_shows_environment_workspace_and_branch() {
    let folder = PathBuf::from("C:/data/ricochet");
    let mut workspace = sample_workspace();
    workspace.environments = vec![ApiEnvironment { id: "dev".into(), name: "Dev".into(), ..Default::default() }];
    workspace.active_environment_id = Some("dev".into());

    let bar = status_bar(&workspace, Some(&folder), Some("main".into()));
    assert_eq!(bar.environment, "Dev");
    assert_eq!(bar.workspace, folder.display().to_string());
    assert_eq!(bar.branch.as_deref(), Some("main"));

    let no_repo = status_bar(&workspace, Some(&folder), None);
    assert_eq!(no_repo.branch, None);
}

#[test]
fn folder_comes_from_the_argument_first_then_the_environment_variable() {
    assert_eq!(folder_from(Some("D:/a"), Some("D:/b")), Some(PathBuf::from("D:/a")));
    assert_eq!(folder_from(None, Some("D:/b")), Some(PathBuf::from("D:/b")));
    assert_eq!(folder_from(Some("  "), Some("D:/b")), Some(PathBuf::from("D:/b")));
    assert_eq!(folder_from(None, Some("")), None);
    assert_eq!(folder_from(None, None), None);
}

/// A new empty directory under the system temp folder, removed by the caller.
fn temp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("ricochet-app-{tag}-{}-{nanos}", process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_missing_folder_is_reported_and_not_created() {
    let dir = temp_dir("missing").join("not-here");
    let model = ShellModel::load(Some(dir.clone()));
    assert!(model.rows.is_empty());
    assert!(model.error.as_deref().unwrap().contains("was not found"));
    assert!(!dir.exists(), "the shell must never create the folder it was asked to open");
    std::fs::remove_dir_all(dir.parent().unwrap()).ok();
}

#[test]
fn no_folder_is_reported_with_a_hint() {
    let model = ShellModel::load(None);
    assert!(model.rows.is_empty());
    assert!(model.error.as_deref().unwrap().contains("RICOCHET_DATA"));
}

#[test]
fn the_shell_reads_a_workspace_folder_through_the_store() {
    let dir = temp_dir("read");
    WorkspaceFolder::new(&dir).save(&sample_workspace()).unwrap();

    let model = ShellModel::load(Some(dir.clone()));
    let labels: Vec<&str> = model.rows.iter().map(|row| row.label.as_str()).collect();
    assert_eq!(model.error, None);
    assert_eq!(labels, vec!["Shop", "Orders", "List orders", "Place order", "Ping", "Health", "Status"]);
    assert_eq!(model.status.environment, NO_ENVIRONMENT);
    assert_eq!(model.status.workspace, dir.display().to_string());
    assert!(model.issues.is_empty());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_unreadable_file_is_left_alone_and_the_rest_still_loads() {
    let dir = temp_dir("issues");
    WorkspaceFolder::new(&dir).save(&sample_workspace()).unwrap();
    let collections: &Path = &dir.join("collections");
    std::fs::create_dir_all(collections).unwrap();
    std::fs::write(collections.join("broken.json"), "{ not json").unwrap();

    let model = ShellModel::load(Some(dir.clone()));
    assert_eq!(model.error, None);
    assert_eq!(model.rows.len(), 7);
    assert!(collections.join("broken.json").exists(), "a file the shell cannot read must be left on disk");

    std::fs::remove_dir_all(&dir).ok();
}
