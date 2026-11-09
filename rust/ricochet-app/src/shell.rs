//! What the shell window shows, loaded from one Workspace folder. Pure enough to test without a window.

use std::path::{Path, PathBuf};

use ricochet_core::models::Workspace;
use ricochet_core::store::{WorkspaceFolder, WorkspaceIssue};

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

impl ShellModel {
    /// Reads the folder through the store. A missing folder is reported, never created.
    pub fn load(folder: Option<PathBuf>) -> Self {
        let Some(path) = folder else {
            return Self::failed(None, "No workspace folder. Pass a folder path on the command line or set RICOCHET_DATA.");
        };
        if !path.is_dir() {
            let message = format!("The workspace folder {} was not found.", path.display());
            return Self::failed(Some(path), &message);
        }
        match WorkspaceFolder::new(&path).load() {
            Ok((workspace, issues)) => {
                let branch = read_branch(&path);
                Self::from_parts(Some(path), workspace, issues, branch)
            }
            Err(error) => {
                let message = format!("The workspace could not be read: {error}");
                Self::failed(Some(path), &message)
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
