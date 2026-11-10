//! Tests for the git branch reader, ported from the branch part of `GitStatusReader.cs`. Each test creates its own
//! repository with `git init` in a temporary folder and removes it afterwards. Nothing here uses the network: the
//! repositories have no remote.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use pretty_assertions::assert_eq;
use ricochet_core::git_branch::*;
use ricochet_core::models::new_id;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("ricochet-git-{tag}-{}", new_id()));
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

/// Runs git in `folder` with a fixed identity, and returns its trimmed stdout. Panics if git fails.
fn git(folder: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(folder)
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "-c", "commit.gpgsign=false"])
        .args(arguments)
        .output()
        .expect("git is installed for these tests");
    assert!(output.status.success(), "git {arguments:?} failed: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// A repository on `branch` with one empty commit, so that HEAD resolves.
fn repository_with_commit(dir: &TempDir, branch: &str) {
    git(dir.path(), &["init", "--quiet", &format!("--initial-branch={branch}")]);
    git(dir.path(), &["commit", "--quiet", "--allow-empty", "-m", "first"]);
}

#[test]
fn a_folder_that_is_not_a_repository_has_no_branch() {
    let dir = TempDir::new("not-repo");

    assert_eq!(current_branch(dir.path()), None);
}

#[test]
fn a_repository_with_no_commits_has_no_branch() {
    let dir = TempDir::new("no-commits");
    git(dir.path(), &["init", "--quiet", "--initial-branch=main"]);

    assert_eq!(current_branch(dir.path()), None, "HEAD does not resolve until the first commit");
}

#[test]
fn the_branch_name_is_returned() {
    let dir = TempDir::new("branch");
    repository_with_commit(&dir, "main");

    assert_eq!(current_branch(dir.path()), Some("main".to_string()));
}

#[test]
fn a_branch_with_slashes_keeps_its_full_name() {
    let dir = TempDir::new("slashes");
    repository_with_commit(&dir, "main");
    git(dir.path(), &["switch", "--quiet", "-c", "feature/status-bar"]);

    assert_eq!(current_branch(dir.path()), Some("feature/status-bar".to_string()));
}

#[test]
fn a_detached_head_gives_the_short_commit_hash() {
    let dir = TempDir::new("detached");
    repository_with_commit(&dir, "main");
    git(dir.path(), &["commit", "--quiet", "--allow-empty", "-m", "second"]);
    git(dir.path(), &["checkout", "--quiet", "--detach"]);
    let short = git(dir.path(), &["rev-parse", "--short", "HEAD"]);

    assert_eq!(current_branch(dir.path()), Some(short));
}

#[test]
fn a_subfolder_of_the_repository_reports_the_same_branch() {
    let dir = TempDir::new("subfolder");
    repository_with_commit(&dir, "main");
    let nested = dir.path().join("collections").join("billing");
    fs::create_dir_all(&nested).unwrap();

    assert_eq!(current_branch(&nested), Some("main".to_string()));
}

#[test]
fn a_missing_folder_has_no_branch() {
    let dir = TempDir::new("missing-folder");
    let missing = dir.path().join("does-not-exist");

    assert_eq!(current_branch(&missing), None);
}

#[test]
fn a_git_program_that_does_not_exist_gives_no_branch() {
    let dir = TempDir::new("no-git");
    repository_with_commit(&dir, "main");
    let missing_git = dir.path().join("no-such-git.exe");

    assert_eq!(read_branch(&missing_git, dir.path()), None);
}

#[test]
fn read_branch_with_the_installed_git_matches_current_branch() {
    let dir = TempDir::new("read-branch");
    repository_with_commit(&dir, "release");
    let git_path = PathBuf::from("git");

    assert_eq!(read_branch(&git_path, dir.path()), Some("release".to_string()));
}
