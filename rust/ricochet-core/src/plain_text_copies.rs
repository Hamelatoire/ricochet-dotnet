//! Older plain-text copies of the person's data that may still hold credentials, so they can be offered for deletion.
//! Port of `FindPlainTextCopies` and `DeletePlainTextCopies` in `WorkspaceStore.cs`.
//!
//! Two files qualify, and nothing else ever does:
//! - `workspace.json.bak` in the personal directory, left behind when the single-file workspace was migrated.
//! - `workspace.json` in the app's previous `PostmanLight` folder, only for the normal personal directory. A custom
//!   one, such as `RICOCHET_DATA` in a test, must never reach outside itself.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::store::{LEGACY_BACKUP_SUFFIX, LEGACY_FILE_NAME};

/// The folder the app kept its data in under its previous name.
const OLD_APP_FOLDER_NAME: &str = "PostmanLight";

/// Where the old copies may be, for one personal directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlainTextCopies {
    personal_dir: PathBuf,
    old_app_copy: Option<PathBuf>,
}

impl PlainTextCopies {
    /// `old_app_copy` is the previous app's file, from [`default_old_app_copy`], or None to leave it out.
    pub fn new(personal_dir: impl Into<PathBuf>, old_app_copy: Option<PathBuf>) -> Self {
        Self { personal_dir: personal_dir.into(), old_app_copy }
    }

    /// The copies that exist now, the previous app's first.
    pub fn find(&self) -> Vec<PathBuf> {
        let backup = self.personal_dir.join(format!("{LEGACY_FILE_NAME}{LEGACY_BACKUP_SUFFIX}"));
        self.old_app_copy.iter().cloned().chain([backup]).filter(|path| path.is_file()).collect()
    }

    /// Permanently deletes the listed copies that [`PlainTextCopies::find`] still reports, and nothing else: a path
    /// that was not listed, or is not one of the copies, is left alone. Returns a problem for each one that could not
    /// be deleted.
    pub fn delete(&self, listed: &[PathBuf]) -> Vec<String> {
        let mut problems = Vec::new();
        for path in self.find().into_iter().filter(|path| listed.contains(path)) {
            if let Err(error) = self.delete_one(&path) {
                problems.push(format!("{}: {error}", path.display()));
            }
        }
        problems
    }

    fn delete_one(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)?;

        // The old app's folder held only this file, so don't leave it behind empty.
        if self.old_app_copy.as_deref() == Some(path) {
            if let Some(folder) = path.parent() {
                if fs::read_dir(folder)?.next().is_none() {
                    fs::remove_dir(folder)?;
                }
            }
        }
        Ok(())
    }
}

/// Where the app's previous name kept its data: None when `override_path` (`RICOCHET_DATA`) is set and not empty, so
/// a custom personal directory never reaches outside itself. Takes its inputs as values so it can be tested.
pub fn resolve_old_app_copy(override_path: Option<&str>, app_data: Option<&Path>) -> Option<PathBuf> {
    if override_path.is_some_and(|path| !path.is_empty()) {
        return None;
    }
    app_data.map(|directory| directory.join(OLD_APP_FOLDER_NAME).join(LEGACY_FILE_NAME))
}

/// The previous app's copy for this process, read from `RICOCHET_DATA` and the app data folder.
pub fn default_old_app_copy() -> Option<PathBuf> {
    let override_path = std::env::var("RICOCHET_DATA").ok();
    resolve_old_app_copy(override_path.as_deref(), crate::personal_state::app_data_directory().as_deref())
}
