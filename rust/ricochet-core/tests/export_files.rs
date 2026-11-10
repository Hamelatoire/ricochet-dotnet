use std::fs;
use std::path::{Path, PathBuf};

use pretty_assertions::assert_eq;
use ricochet_core::export_files::*;
use ricochet_core::models::Collection;

/// A throwaway folder under the system temp directory, removed when the test ends.
struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("ricochet-export-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn workspace(&self) -> PathBuf {
        let workspace = self.0.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        workspace
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn named(name: &str) -> Collection {
    Collection { name: name.into(), ..Default::default() }
}

#[test]
fn find_collection_matches_the_name_ignoring_case() {
    let collections = [named("Shop"), named("Billing")];

    let found = find_collection(&collections, "shop").unwrap();

    assert_eq!(found.name, "Shop");
}

#[test]
fn find_collection_names_the_collections_it_has_when_none_matches() {
    let collections = [named("Shop")];

    let error = find_collection(&collections, "Nope").unwrap_err();

    assert!(error.to_string().contains("No collection is named \"Nope\""), "{error}");
    assert!(error.to_string().contains("Shop"), "{error}");
}

#[test]
fn find_collection_says_the_workspace_has_no_collections_when_it_is_empty() {
    let error = find_collection(&[], "Nope").unwrap_err();

    assert_eq!(error.to_string(), "No collection is named \"Nope\". The workspace has no collections.");
}

#[test]
fn find_collection_leaves_out_unnamed_collections_from_the_list() {
    let collections = [named(""), named("Shop")];

    let error = find_collection(&collections, "Nope").unwrap_err();

    assert_eq!(error.to_string(), "No collection is named \"Nope\". Collections: Shop.");
}

#[test]
fn find_collection_refuses_two_collections_with_the_same_name() {
    let collections = [named("Shop"), named("shop")];

    let error = find_collection(&collections, "Shop").unwrap_err();

    assert!(error.to_string().contains("More than one"), "{error}");
}

#[test]
fn write_creates_a_new_file_as_utf8_without_a_byte_order_mark() {
    let root = TempRoot::new();
    let target = root.path().join("out.json");

    write(&target, "{\"name\":\"café\"}", &root.workspace(), false).unwrap();

    let bytes = fs::read(&target).unwrap();
    assert!(!(bytes.len() >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF));
    assert_eq!(String::from_utf8(bytes).unwrap(), "{\"name\":\"café\"}");
}

#[test]
fn write_refuses_an_existing_file_without_force_and_leaves_it_unchanged() {
    let root = TempRoot::new();
    let target = root.path().join("out.json");
    fs::write(&target, "original").unwrap();

    let error = write(&target, "new", &root.workspace(), false).unwrap_err();

    assert!(error.to_string().contains("--force"), "{error}");
    assert_eq!(fs::read_to_string(&target).unwrap(), "original");
}

#[test]
fn write_overwrites_an_existing_file_with_force() {
    let root = TempRoot::new();
    let target = root.path().join("out.json");
    fs::write(&target, "original").unwrap();

    write(&target, "new", &root.workspace(), true).unwrap();

    assert_eq!(fs::read_to_string(&target).unwrap(), "new");
}

#[test]
fn write_refuses_a_path_inside_the_workspace_even_with_force() {
    let root = TempRoot::new();
    let workspace = root.workspace();

    assert!(write(&workspace.join("export.json"), "x", &workspace, true).is_err());
    assert!(write(&workspace.join("sub").join("export.json"), "x", &workspace, true).is_err());
    assert!(!workspace.join("export.json").exists());
}

#[test]
fn write_refuses_a_path_that_climbs_back_into_the_workspace() {
    let root = TempRoot::new();
    let workspace = root.workspace();
    let sneaky = workspace.join("..").join("workspace").join("export.json");

    assert!(write(&sneaky, "x", &workspace, true).is_err());
    assert!(!workspace.join("export.json").exists());
}

#[test]
fn write_allows_a_sibling_folder_whose_name_starts_with_the_workspace_name() {
    let root = TempRoot::new();
    let workspace = root.workspace();
    let sibling = root.path().join("workspace-exports");
    fs::create_dir_all(&sibling).unwrap();

    write(&sibling.join("export.json"), "x", &workspace, false).unwrap();

    assert!(sibling.join("export.json").is_file());
}

#[test]
fn write_refuses_a_folder_as_the_target() {
    let root = TempRoot::new();
    let folder = root.path().join("exports");
    fs::create_dir_all(&folder).unwrap();

    assert!(write(&folder, "x", &root.workspace(), true).is_err());
}

#[test]
fn write_reports_a_missing_folder_as_an_export_error() {
    let root = TempRoot::new();
    let target = root.path().join("missing").join("out.json");

    let error = write(&target, "x", &root.workspace(), false).unwrap_err();

    assert!(error.to_string().starts_with("Could not write"), "{error}");
}

#[test]
fn is_inside_counts_the_folder_itself_and_anything_below_it() {
    let folder = Path::new(if cfg!(windows) { "C:\\data\\workspace" } else { "/data/workspace" });

    assert!(is_inside(folder, folder));
    assert!(is_inside(&folder.join("a").join("b.json"), folder));
    assert!(!is_inside(&folder.with_file_name("workspace-exports").join("b.json"), folder));
}
