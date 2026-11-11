//! The Settings page's Workspace folder row (C# `OpenWorkspaceFolderAsync`, `PendingCopyFolder`, ADR 0031): which
//! folder opens at start, what state.json remembers, and what a picked folder leads to. Folders are throwaway temp
//! directories, never a real Workspace.

use std::fs;
use std::path::PathBuf;
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use pretty_assertions::assert_eq;
use ricochet_app::shell::{FolderChoice, FolderStep, folder_step, folder_to_remember, same_folder, start_folder};
use ricochet_core::models::Workspace;
use ricochet_core::store::WorkspaceFolder;

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("ricochet-app-folder-{tag}-{}-{nanos}", process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn workspace_in(dir: &PathBuf) {
    let mut store = WorkspaceFolder::new(dir);
    store.load().unwrap();
    store.save(&Workspace::default()).unwrap();
}

#[test]
fn the_argument_wins_over_the_remembered_folder() {
    let remembered = temp_dir("remembered-arg");
    let (folder, note) = start_folder(Some("given"), remembered.to_str(), Some("env"));
    assert_eq!(folder, Some(PathBuf::from("given")));
    assert_eq!(note, None);
}

#[test]
fn without_an_argument_the_remembered_folder_opens() {
    let remembered = temp_dir("remembered");
    let (folder, note) = start_folder(None, remembered.to_str(), Some("env"));
    assert_eq!(folder, Some(remembered));
    assert_eq!(note, None);

    let (folder, _) = start_folder(Some("  "), None, Some("env"));
    assert_eq!(folder, Some(PathBuf::from("env")), "a blank argument and nothing remembered fall back to RICOCHET_DATA");
}

#[test]
fn a_remembered_folder_that_is_gone_is_reported_and_not_opened() {
    let gone = temp_dir("gone").join("not-here");
    let (folder, note) = start_folder(None, gone.to_str(), Some("env"));
    assert_eq!(folder, Some(PathBuf::from("env")));
    assert!(note.unwrap().contains("was not found"));
}

#[test]
fn the_default_folder_is_not_remembered() {
    let default = PathBuf::from("data");
    assert_eq!(folder_to_remember(&PathBuf::from("data/"), Some(&default)), None);
    assert_eq!(folder_to_remember(&PathBuf::from("other"), Some(&default)), Some(PathBuf::from("other")));
    assert_eq!(folder_to_remember(&PathBuf::from("other"), None), Some(PathBuf::from("other")));
}

#[test]
fn same_folder_ignores_a_trailing_separator() {
    assert!(same_folder(&PathBuf::from("a/b"), &PathBuf::from("a/b/")));
    assert!(!same_folder(&PathBuf::from("a/b"), &PathBuf::from("a/c")));
}

#[test]
fn open_switches_to_a_folder_that_holds_a_workspace() {
    let dir = temp_dir("open-workspace");
    workspace_in(&dir);
    assert_eq!(folder_step(FolderChoice::Open, &dir, None), FolderStep::Open(dir.clone()));
}

#[test]
fn open_asks_before_copying_into_an_empty_folder() {
    let dir = temp_dir("open-empty");
    assert_eq!(folder_step(FolderChoice::Open, &dir, None), FolderStep::ConfirmCopy(dir.clone()));
}

#[test]
fn open_refuses_a_folder_with_other_files() {
    let dir = temp_dir("open-other");
    fs::write(dir.join("notes.txt"), "mine").unwrap();
    assert!(matches!(folder_step(FolderChoice::Open, &dir, None), FolderStep::Refuse(text) if text.contains("doesn't hold a workspace")));
}

#[test]
fn copy_goes_only_into_an_empty_folder() {
    let empty = temp_dir("copy-empty");
    assert_eq!(folder_step(FolderChoice::CopyTo, &empty, None), FolderStep::Copy(empty.clone()));

    let missing = temp_dir("copy-missing").join("new");
    assert_eq!(folder_step(FolderChoice::CopyTo, &missing, None), FolderStep::Copy(missing.clone()));

    let other = temp_dir("copy-other");
    fs::write(other.join("notes.txt"), "mine").unwrap();
    assert!(matches!(folder_step(FolderChoice::CopyTo, &other, None), FolderStep::Refuse(text) if text.contains("not empty")));

    let workspace = temp_dir("copy-workspace");
    workspace_in(&workspace);
    assert!(matches!(folder_step(FolderChoice::CopyTo, &workspace, None), FolderStep::Refuse(text) if text.contains("Open folder")));
}

#[test]
fn the_folder_already_open_is_refused() {
    let dir = temp_dir("current");
    workspace_in(&dir);
    for choice in [FolderChoice::Open, FolderChoice::CopyTo] {
        assert!(matches!(folder_step(choice, &dir, Some(&dir)), FolderStep::Refuse(text) if text.contains("already open")));
    }
}
