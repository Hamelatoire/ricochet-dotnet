//! Tests for finding and deleting old plain-text copies. Each test works in its own temporary folder only.

use std::fs;
use std::path::{Path, PathBuf};

use pretty_assertions::assert_eq;
use ricochet_core::plain_text_copies::*;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let id = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("ricochet-copies-{tag}-{}-{id}", std::process::id()));
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

/// A personal directory and an app data folder side by side, with the old app's copy inside the latter.
fn layout(dir: &TempDir) -> (PathBuf, PathBuf) {
    let personal = dir.path().join("Ricochet");
    fs::create_dir_all(&personal).unwrap();
    let old = dir.path().join("PostmanLight").join("workspace.json");
    (personal, old)
}

fn write(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, r#"{"token":"plain"}"#).unwrap();
}

#[test]
fn finds_nothing_when_there_are_no_copies() {
    let dir = TempDir::new("none");
    let (personal, old) = layout(&dir);
    assert!(PlainTextCopies::new(&personal, Some(old)).find().is_empty());
}

#[test]
fn finds_the_backup_and_the_old_app_copy_old_one_first() {
    let dir = TempDir::new("both");
    let (personal, old) = layout(&dir);
    let backup = personal.join("workspace.json.bak");
    write(&backup);
    write(&old);

    assert_eq!(PlainTextCopies::new(&personal, Some(old.clone())).find(), vec![old, backup]);
}

#[test]
fn other_files_never_qualify() {
    let dir = TempDir::new("others");
    let (personal, _) = layout(&dir);
    for name in ["workspace.json", "state.json", "history.json", "secrets.dat", "other.bak"] {
        write(&personal.join(name));
    }
    assert!(PlainTextCopies::new(&personal, None).find().is_empty());
}

#[test]
fn a_folder_named_like_the_backup_is_not_a_copy() {
    let dir = TempDir::new("folder");
    let (personal, _) = layout(&dir);
    fs::create_dir_all(personal.join("workspace.json.bak")).unwrap();
    assert!(PlainTextCopies::new(&personal, None).find().is_empty());
}

#[test]
fn delete_removes_the_listed_copies_and_the_emptied_old_app_folder() {
    let dir = TempDir::new("delete");
    let (personal, old) = layout(&dir);
    let backup = personal.join("workspace.json.bak");
    write(&backup);
    write(&old);
    write(&personal.join("state.json"));
    let copies = PlainTextCopies::new(&personal, Some(old.clone()));

    let problems = copies.delete(&copies.find());

    assert!(problems.is_empty(), "{problems:?}");
    assert!(copies.find().is_empty());
    assert!(!old.parent().unwrap().exists());
    assert!(personal.join("state.json").exists());
}

#[test]
fn delete_keeps_the_old_app_folder_when_it_holds_anything_else() {
    let dir = TempDir::new("keep-folder");
    let (personal, old) = layout(&dir);
    write(&old);
    let other = old.parent().unwrap().join("notes.txt");
    write(&other);
    let copies = PlainTextCopies::new(&personal, Some(old.clone()));

    assert!(copies.delete(&copies.find()).is_empty());

    assert!(!old.exists());
    assert!(other.exists());
}

#[test]
fn delete_leaves_alone_copies_that_were_not_listed() {
    let dir = TempDir::new("unlisted");
    let (personal, old) = layout(&dir);
    let backup = personal.join("workspace.json.bak");
    write(&backup);
    write(&old);
    let copies = PlainTextCopies::new(&personal, Some(old.clone()));

    assert!(copies.delete(std::slice::from_ref(&backup)).is_empty());

    assert!(!backup.exists());
    assert!(old.exists());
}

#[test]
fn delete_never_touches_a_listed_path_that_is_not_a_copy() {
    let dir = TempDir::new("not-a-copy");
    let (personal, _) = layout(&dir);
    let state = personal.join("state.json");
    write(&state);
    let copies = PlainTextCopies::new(&personal, None);

    assert!(copies.delete(&[state.clone()]).is_empty());

    assert!(state.exists());
}

#[test]
fn old_app_copy_is_left_out_for_a_custom_personal_directory() {
    let app_data = Path::new("appdata");
    assert_eq!(resolve_old_app_copy(Some("custom"), Some(app_data)), None);
    assert_eq!(
        resolve_old_app_copy(None, Some(app_data)),
        Some(app_data.join("PostmanLight").join("workspace.json"))
    );
    assert_eq!(
        resolve_old_app_copy(Some(""), Some(app_data)),
        Some(app_data.join("PostmanLight").join("workspace.json"))
    );
    assert_eq!(resolve_old_app_copy(None, None), None);
}
