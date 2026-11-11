//! What the shell window shows, loaded from one Workspace folder. Pure enough to test without a window.

use std::path::{Path, PathBuf};

use ricochet_core::models::Workspace;
use ricochet_core::store::{WorkspaceFolder, WorkspaceIssue, is_empty_folder, looks_like_workspace};

use crate::status::{StatusBar, read_branch, status_bar};
use crate::tree::{TreeRow, build_tree};

/// Everything the window draws. Built once at start; nothing here is written back.
#[derive(Debug, Clone, PartialEq)]
pub struct ShellModel {
    pub folder: Option<PathBuf>,
    pub workspace: Workspace,
    /// Files the store could not read. They are left alone on disk.
    pub issues: Vec<WorkspaceIssue>,
    pub rows: Vec<TreeRow>,
    pub status: StatusBar,
    /// Why there is no Workspace to show, when there isn't one.
    pub error: Option<String>,
}

/// The folder to open: the first command-line argument, else `RICOCHET_DATA`. Blank values count as absent.
pub fn folder_from(arg: Option<&str>, env: Option<&str>) -> Option<PathBuf> {
    [arg, env]
        .into_iter()
        .flatten()
        .find(|value| !value.trim().is_empty())
        .map(PathBuf::from)
}

/// The folder to open at start: the command-line argument, else the folder opened last time (`workspaceFolder` in
/// state.json), else `RICOCHET_DATA`. A remembered folder that is gone, perhaps on an unplugged drive, is skipped
/// with a note to show, rather than recreated empty, as C# `WorkspaceStore.Load` does.
pub fn start_folder(arg: Option<&str>, remembered: Option<&str>, env: Option<&str>) -> (Option<PathBuf>, Option<String>) {
    let blank = |value: &Option<&str>| value.is_none_or(|text| text.trim().is_empty());
    if !blank(&arg) {
        return (folder_from(arg, None), None);
    }
    let fallback = folder_from(None, env);
    match remembered.filter(|text| !text.trim().is_empty()).map(PathBuf::from) {
        Some(path) if path.is_dir() => (Some(path), None),
        Some(path) => {
            let note = format!("The workspace folder {} was not found, so it was not opened.", path.display());
            (fallback, Some(note))
        }
        None => (fallback, None),
    }
}

/// What state.json should remember after a folder is opened: the folder, or None when it is the default one, so that
/// moving the personal folder keeps working, as C# `RememberedFolder` does.
pub fn folder_to_remember(opened: &Path, default: Option<&Path>) -> Option<PathBuf> {
    match default {
        Some(default) if same_folder(opened, default) => None,
        _ => Some(opened.to_path_buf()),
    }
}

/// Whether two paths name the same folder, ignoring a trailing separator, and case on Windows.
pub fn same_folder(a: &Path, b: &Path) -> bool {
    let text = |path: &Path| {
        let text = path.to_string_lossy().trim_end_matches(['/', '\\']).to_string();
        if cfg!(windows) { text.replace('/', "\\").to_lowercase() } else { text }
    };
    text(a) == text(b)
}

/// The two buttons on the Settings page's Workspace folder row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderChoice {
    /// Open folder…: switch to the Workspace in another folder.
    Open,
    /// Copy to new folder…: write this Workspace into an empty folder and switch to the copy.
    CopyTo,
}

/// What happens with the folder picked for a [`FolderChoice`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderStep {
    /// The folder holds a Workspace: open it.
    Open(PathBuf),
    /// Opening an empty folder: ask before copying this Workspace there, as C# `PendingCopyFolder` does.
    ConfirmCopy(PathBuf),
    /// Copy this Workspace into the folder, which is empty.
    Copy(PathBuf),
    /// Nothing happens, for the reason given.
    Refuse(String),
}

/// Decides what a picked folder leads to. Nothing is written into a folder that has files in it, and a folder that
/// already holds a Workspace is only ever opened, never copied over.
pub fn folder_step(choice: FolderChoice, picked: &Path, current: Option<&Path>) -> FolderStep {
    let shown = picked.display();
    if current.is_some_and(|current| same_folder(current, picked)) {
        return FolderStep::Refuse(format!("{shown} is the workspace that is already open."));
    }
    let is_workspace = looks_like_workspace(picked);
    let is_empty = match is_empty_folder(picked) {
        Ok(empty) => empty,
        Err(error) => return FolderStep::Refuse(format!("{shown} could not be read: {error}")),
    };
    match choice {
        FolderChoice::Open if is_workspace => FolderStep::Open(picked.to_path_buf()),
        FolderChoice::Open if is_empty => FolderStep::ConfirmCopy(picked.to_path_buf()),
        FolderChoice::Open => FolderStep::Refuse(format!(
            "{shown} doesn't hold a workspace. To copy this workspace into a new folder, use Copy to new folder… with an empty folder."
        )),
        FolderChoice::CopyTo if is_workspace => {
            FolderStep::Refuse(format!("{shown} already holds a workspace. Use Open folder… to open it."))
        }
        FolderChoice::CopyTo if is_empty => FolderStep::Copy(picked.to_path_buf()),
        FolderChoice::CopyTo => {
            FolderStep::Refuse(format!("{shown} is not empty. A workspace is only copied into an empty folder."))
        }
    }
}

/// The question asked before copying into an empty folder picked with Open folder…, as in C# `PendingCopyMessage`.
pub fn pending_copy_message(path: &Path) -> String {
    format!(
        "{} doesn't hold a workspace yet. Copy your current workspace there? The current workspace stays where it is.",
        path.display()
    )
}

impl ShellModel {
    /// Reads the folder through the store. A missing folder is reported, never created.
    pub fn load(folder: Option<PathBuf>) -> Self {
        Self::open(folder).0
    }

    /// As [`ShellModel::load`], and also hands back the store that read the folder. Saving must go through that
    /// same store, because it knows which files it understood and so may change. None when nothing was read.
    pub fn open(folder: Option<PathBuf>) -> (Self, Option<WorkspaceFolder>) {
        let Some(path) = folder else {
            let message = "No workspace folder. Pass a folder path on the command line or set RICOCHET_DATA.";
            return (Self::failed(None, message), None);
        };
        if !path.is_dir() {
            let message = format!("The workspace folder {} was not found.", path.display());
            return (Self::failed(Some(path), &message), None);
        }
        let mut store = WorkspaceFolder::new(&path);
        match store.load() {
            Ok((workspace, issues)) => {
                let branch = read_branch(&path);
                (Self::from_parts(Some(path), workspace, issues, branch), Some(store))
            }
            Err(error) => {
                let message = format!("The workspace could not be read: {error}");
                (Self::failed(Some(path), &message), None)
            }
        }
    }

    /// Builds the model from a Workspace already read. The sidebar never shows hidden items.
    pub fn from_parts(folder: Option<PathBuf>, workspace: Workspace, issues: Vec<WorkspaceIssue>, branch: Option<String>) -> Self {
        let rows = build_tree(&workspace, false);
        let status = status_bar(&workspace, folder.as_deref(), branch);
        Self { folder, workspace, issues, rows, status, error: None }
    }

    fn failed(folder: Option<PathBuf>, message: &str) -> Self {
        let workspace = Workspace::default();
        let status = status_bar(&workspace, folder.as_deref(), None);
        Self { folder, workspace, issues: Vec::new(), rows: Vec::new(), status, error: Some(message.to_string()) }
    }

    pub fn folder_path(&self) -> Option<&Path> {
        self.folder.as_deref()
    }
}
