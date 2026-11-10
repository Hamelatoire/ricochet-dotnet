//! Tests for the git clone port (ADR 0034). Clones come from local bare repositories in a temp folder, and
//! the real git CLI runs. Nothing touches the network.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

use ricochet_core::git_clone::{GitCloneResult, check_target_folder, check_url, clone_repository};
use uuid::Uuid;

/// A temp folder that is removed on drop. Git marks its object files read-only, which stops Windows from deleting
/// them, so the permissions are cleared first.
struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("ricochet-clone-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).expect("create temp root");
        Self(path)
    }

    fn child(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// A new empty folder under the root.
    fn empty_folder(&self, name: &str) -> PathBuf {
        let folder = self.child(name);
        fs::create_dir_all(&folder).expect("create folder");
        folder
    }

    /// A bare repository with one commit that holds the given files.
    fn make_remote(&self, name: &str, files: &[&str]) -> PathBuf {
        let remote = self.child(name);
        let work = self.child(&format!("{name}-work"));
        fs::create_dir_all(&remote).expect("create remote");
        git(&remote, &["init", "-q", "--bare", "-b", "main"]);

        fs::create_dir_all(&work).expect("create work tree");
        git(&work, &["init", "-q", "-b", "main"]);
        for file in files {
            let path = work.join(file);
            fs::create_dir_all(path.parent().expect("file has a parent")).expect("create parent");
            fs::write(&path, "{}").expect("write file");
        }
        git(&work, &["add", "-A"]);
        git(&work, &["-c", "commit.gpgsign=false", "commit", "-q", "-m", "initial"]);
        git(&work, &["push", "-q", remote.to_str().expect("utf-8 path"), "main"]);
        remote
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        clear_read_only(&self.0);
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn clear_read_only(path: &Path) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        if let Ok(metadata) = fs::metadata(&entry_path) {
            let mut permissions = metadata.permissions();
            permissions.set_readonly(false);
            let _ = fs::set_permissions(&entry_path, permissions);
        }
        if entry_path.is_dir() {
            clear_read_only(&entry_path);
        }
    }
}

fn git(folder: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .current_dir(folder)
        .args(arguments)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.test")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.test")
        .output()
        .expect("git must be installed to run these tests");
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn clone(url: &str, folder: &Path) -> GitCloneResult {
    clone_repository(url, folder.to_str().expect("utf-8 path"), &AtomicBool::new(false))
}

#[test]
fn cloning_a_workspace_repository_reports_a_workspace() {
    let root = TempRoot::new();
    let remote = root.make_remote("workspace.git", &["ricochet.json", "collections/a/collection.json"]);
    let target = root.empty_folder("target");

    let result = clone(remote.to_str().unwrap(), &target);

    assert!(result.succeeded, "{}", result.message);
    assert!(result.is_workspace, "{}", result.message);
    assert!(target.join("ricochet.json").is_file());
}

#[test]
fn cloning_a_repository_that_is_not_a_workspace_says_so_and_keeps_the_files() {
    let root = TempRoot::new();
    let remote = root.make_remote("notes.git", &["README.md"]);
    let target = root.empty_folder("target");

    let result = clone(remote.to_str().unwrap(), &target);

    assert!(result.succeeded, "{}", result.message);
    assert!(!result.is_workspace);
    assert!(result.message.contains("not a Ricochet workspace"), "{}", result.message);
    assert!(target.join("README.md").is_file());
}

#[test]
fn a_missing_repository_fails_and_reports_git_s_message() {
    let root = TempRoot::new();
    let target = root.empty_folder("target");
    let missing = root.child("does-not-exist.git");

    let result = clone(missing.to_str().unwrap(), &target);

    assert!(!result.succeeded);
    assert!(!result.is_workspace);
    assert!(result.message.starts_with("git clone failed"), "{}", result.message);
}

#[test]
fn a_non_empty_folder_is_refused_and_left_alone() {
    let root = TempRoot::new();
    let remote = root.make_remote("workspace2.git", &["ricochet.json"]);
    let target = root.empty_folder("target");
    let existing = target.join("mine.txt");
    fs::write(&existing, "keep me").unwrap();

    let result = clone(remote.to_str().unwrap(), &target);

    assert!(!result.succeeded);
    assert!(result.message.contains("not empty"), "{}", result.message);
    assert_eq!(fs::read_to_string(&existing).unwrap(), "keep me");
    assert!(!target.join("ricochet.json").exists());
}

#[test]
fn a_target_folder_must_exist_and_be_empty() {
    let root = TempRoot::new();
    let empty = root.empty_folder("empty");
    let missing = root.child("missing");
    let file = root.child("afile.txt");
    fs::write(&file, "x").unwrap();
    let full = root.empty_folder("full");
    fs::write(full.join("a.txt"), "x").unwrap();

    assert_eq!(check_target_folder(empty.to_str().unwrap()), None);
    assert!(check_target_folder(missing.to_str().unwrap()).unwrap().contains("does not exist"));
    assert!(check_target_folder(file.to_str().unwrap()).unwrap().contains("is a file"));
    assert!(check_target_folder(full.to_str().unwrap()).unwrap().contains("not empty"));
    assert!(check_target_folder("").is_some());
    assert!(check_target_folder("   ").is_some());
}

#[test]
fn blank_urls_and_urls_that_look_like_options_are_refused() {
    assert!(check_url("").is_some());
    assert!(check_url("   ").is_some());
    assert!(check_url("--upload-pack=touch x").is_some());
}

#[test]
fn urls_with_a_password_are_refused_without_echoing_it() {
    let problem = check_url("https://alice:hunter2@example.test/team/api.git").expect("refused");

    assert!(problem.contains("password"), "{problem}");
    assert!(!problem.contains("hunter2"), "{problem}");
}

#[test]
fn transports_that_run_programs_are_refused() {
    assert!(check_url("ext::sh -c echo").is_some());
    assert!(check_url("ftp://example.test/repo.git").is_some());
}

#[test]
fn ordinary_repository_addresses_are_accepted() {
    for url in [
        "https://example.test/team/api.git",
        "ssh://git@example.test/team/api.git",
        "git@example.test:team/api.git",
        r"C:\repos\api.git",
        "file:///srv/repos/api.git",
    ] {
        assert_eq!(check_url(url), None, "{url} should be accepted");
    }
}

#[test]
fn refused_urls_never_reach_git() {
    let root = TempRoot::new();
    let target = root.empty_folder("target");

    let result = clone("https://alice:hunter2@example.test/api.git", &target);

    assert!(!result.succeeded);
    assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
}

#[test]
fn a_clone_that_is_cancelled_fails_and_reports_it() {
    let root = TempRoot::new();
    let remote = root.make_remote("cancel.git", &["ricochet.json"]);
    let target = root.empty_folder("target");
    let cancel = AtomicBool::new(true);

    let result = clone_repository(remote.to_str().unwrap(), target.to_str().unwrap(), &cancel);

    assert!(!result.succeeded);
    assert!(!result.is_workspace);
    assert_eq!(result.message, "git clone failed: The clone was cancelled.");
}
