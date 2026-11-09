//! Port of src/Ricochet.Core/ExportFiles.cs (ADR 0029). Finds the Collection to export and writes export
//! files. An export must never land inside the Workspace folder, because the Workspace is shared through git
//! and a stray export there would be committed.

use std::fmt;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use crate::models::Collection;

/// An export that cannot be written: no such Collection, a file that already exists, or a path inside the Workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportFileError(pub String);

impl fmt::Display for ExportFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ExportFileError {}

/// Finds the one Collection whose name matches `name`, ignoring case.
pub fn find_collection<'a>(collections: &'a [Collection], name: &str) -> Result<&'a Collection, ExportFileError> {
    let matches: Vec<&Collection> = collections.iter().filter(|c| same_name(&c.name, name)).collect();
    match matches.as_slice() {
        [only] => Ok(only),
        [_, _, ..] => Err(ExportFileError(format!(
            "More than one collection is named \"{name}\". Rename one of them and try again."
        ))),
        [] => {
            let names: Vec<&str> = collections.iter().map(|c| c.name.as_str()).filter(|n| !n.is_empty()).collect();
            let known = if names.is_empty() {
                "The workspace has no collections.".to_string()
            } else {
                format!("Collections: {}.", names.join(", "))
            };
            Err(ExportFileError(format!("No collection is named \"{name}\". {known}")))
        }
    }
}

/// Checks a target path without writing. Returns its full path, or an error when it must not be written.
pub fn check(path: &Path, workspace_root: &Path, force: bool) -> Result<PathBuf, ExportFileError> {
    let full = full_path(path);
    if is_inside(&full, &full_path(workspace_root)) {
        return Err(ExportFileError(format!(
            "\"{}\" is inside the workspace folder. Export to a folder outside it.",
            full.display()
        )));
    }

    if full.is_dir() {
        return Err(ExportFileError(format!("\"{}\" is a folder.", full.display())));
    }

    if !force && full.exists() {
        return Err(ExportFileError(format!(
            "\"{}\" already exists. Pass --force to overwrite it.",
            full.display()
        )));
    }

    Ok(full)
}

/// Writes UTF-8 without a byte-order mark. Refuses an existing file unless `force` is set.
pub fn write(path: &Path, contents: &str, workspace_root: &Path, force: bool) -> Result<(), ExportFileError> {
    let full = check(path, workspace_root, force)?;

    let mut options = OpenOptions::new();
    options.write(true);
    if force {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }

    options
        .open(&full)
        .and_then(|mut file| file.write_all(contents.as_bytes()))
        .map_err(|err| ExportFileError(format!("Could not write \"{}\": {err}", full.display())))
}

/// True when the path is the folder itself or anything below it. Compares whole path components, so a sibling
/// such as `workspace-exports` is not inside `workspace`. Case is ignored on Windows.
pub fn is_inside(full: &Path, folder: &Path) -> bool {
    let full: Vec<Component> = full.components().collect();
    let folder: Vec<Component> = folder.components().collect();
    folder.len() <= full.len()
        && folder
            .iter()
            .zip(full.iter())
            .all(|(a, b)| component_eq(a.as_os_str().to_string_lossy().as_ref(), b.as_os_str().to_string_lossy().as_ref()))
}

fn same_name(a: &str, b: &str) -> bool {
    a.to_uppercase() == b.to_uppercase()
}

fn component_eq(a: &str, b: &str) -> bool {
    if cfg!(windows) { a.eq_ignore_ascii_case(b) } else { a == b }
}

/// Makes the path absolute against the current folder and resolves `.` and `..` without touching the disk,
/// so it works for files that do not exist yet.
fn full_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };

    let mut out = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}
