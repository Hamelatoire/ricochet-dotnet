//! Port of `WorkspaceArchive.cs` (ADR 0035, ADR 0091). Exports the shareable files of a Workspace folder as one ZIP,
//! and imports such a ZIP into a new or empty folder.
//!
//! Only files in the shareable layout (see `store::WorkspaceFolder`) are ever included, so personal files, History
//! and Secrets stay out. Any problem means nothing is written: an export that cannot read every shared file writes no
//! archive, and an import that refuses any entry writes no folder.
//!
//! Deliberate differences from the C# port:
//! - Import caps how much it unpacks (`ArchiveLimits`). The C# importer has no cap, so a small ZIP could expand to
//!   a huge amount of memory. Limits are generous for real Workspaces.
//! - Export and import refuse any shared file that holds a Secret value (a `KeyValue` with `is_secret` set and a
//!   non-empty value). The Secret split (`WorkspaceSecrets`) is not ported yet, so a Secret must never reach an archive
//!   through a shared file. The C# export relies on the split alone.
//!
//! Import writes the bytes it validated, as C# does, rather than re-rendering through `WorkspaceFolder::save`. The
//! store takes a Workspace's id from `ricochet.json` only on load, so a save into a new folder would give the
//! imported Workspace a new id.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Seek, Write};
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::models::{ApiRequest, Collection, CollectionItem, KeyValue, Workspace, new_id};
use crate::store::{GITIGNORE_CONTENT, GITIGNORE_FILE_NAME, MARKER_FILE_NAME, WorkspaceFolder, WorkspaceIssue};

const GLOBALS_FILE_NAME: &str = "globals.json";
const COLLECTION_FILE_NAME: &str = "collection.json";
const FOLDER_FILE_NAME: &str = "folder.json";
const COLLECTIONS_DIR: &str = "collections";
const ENVIRONMENTS_DIR: &str = "environments";

/// Personal files that could end up in a shared folder by mistake. They are never exported or imported.
const PERSONAL_FILE_NAMES: [&str; 2] = ["state.json", "history.json"];

const MIB: u64 = 1024 * 1024;

/// What an export or import did. Any problem means nothing was written.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspaceArchiveResult {
    pub files: usize,
    pub problems: Vec<String>,
}

impl WorkspaceArchiveResult {
    pub fn succeeded(&self) -> bool {
        self.problems.is_empty()
    }

    fn ok(files: usize) -> Self {
        Self { files, problems: Vec::new() }
    }

    fn failed(problem: impl Into<String>) -> Self {
        Self { files: 0, problems: vec![problem.into()] }
    }

    fn refused(problems: Vec<String>) -> Self {
        Self { files: 0, problems }
    }
}

/// How much an import may unpack. Entries are measured by the bytes they really unpack to, not by the sizes they
/// declare, because a declared size can lie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveLimits {
    pub max_entries: usize,
    pub max_entry_bytes: u64,
    pub max_total_bytes: u64,
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        Self { max_entries: 5_000, max_entry_bytes: 16 * MIB, max_total_bytes: 100 * MIB }
    }
}

/// Which shareable file a relative path is (see `WorkspaceFolder`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Marker,
    Globals,
    Collection,
    Folder,
    Request,
    Environment,
}

/// Exports the shareable files of a workspace folder as one ZIP at `zip_path`, replacing any archive already there.
pub fn export(workspace_root: &Path, zip_path: &Path) -> WorkspaceArchiveResult {
    if !workspace_root.join(MARKER_FILE_NAME).is_file() {
        return WorkspaceArchiveResult::failed(format!("This folder has no {MARKER_FILE_NAME}, so it is not a workspace."));
    }

    // The store reads and checks every shared file. Issues about files outside the shared layout (such as a personal
    // file that the store also scans) are not ours to report, as in C#.
    let mut folder = WorkspaceFolder::new(workspace_root);
    let (workspace, issues) = match folder.load() {
        Ok(loaded) => loaded,
        Err(error) => return WorkspaceArchiveResult::failed(format!("The workspace could not be read: {error}")),
    };
    let mut problems: Vec<String> = issues.iter().filter(|issue| kind_of(&issue.path).is_some()).map(issue_text).collect();
    problems.extend(secret_problems(&workspace));

    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for name in shared_candidates(workspace_root) {
        if kind_of(&name).is_none() {
            continue;
        }
        match fs::read(name_to_path(workspace_root, &name)) {
            Ok(bytes) => files.push((name, bytes)),
            Err(error) => problems.push(format!("{name}: {error}")),
        }
    }

    // A file that could not be read would make an archive that import refuses, so nothing is written.
    if !problems.is_empty() {
        return WorkspaceArchiveResult::refused(problems);
    }

    files.sort_by(|a, b| a.0.cmp(&b.0));
    match write_zip(zip_path, &files) {
        Ok(()) => WorkspaceArchiveResult::ok(files.len()),
        Err(error) => WorkspaceArchiveResult::failed(format!("The archive could not be written: {error}")),
    }
}

/// Imports the archive's workspace files into `target_folder`, which must not exist or must be empty. Every entry is
/// checked before anything is written.
pub fn import(zip_path: &Path, target_folder: &Path) -> WorkspaceArchiveResult {
    import_with_limits(zip_path, target_folder, ArchiveLimits::default())
}

/// `import` with the unpacking limits given, so tests can use small ones.
pub fn import_with_limits(zip_path: &Path, target_folder: &Path, limits: ArchiveLimits) -> WorkspaceArchiveResult {
    let target = match std::path::absolute(target_folder) {
        Ok(path) => path,
        Err(error) => return WorkspaceArchiveResult::failed(format!("The folder could not be used: {error}")),
    };
    if target.is_file() {
        return WorkspaceArchiveResult::failed("A file already has this name, so the archive was not imported. Choose a new, empty folder.");
    }
    if target.is_dir() && fs::read_dir(&target).map(|mut entries| entries.next().is_some()).unwrap_or(true) {
        return WorkspaceArchiveResult::failed("The folder is not empty, so the archive was not imported. Choose a new, empty folder.");
    }

    let files = match read_entries(zip_path, limits) {
        Ok(files) => files,
        Err(problems) => return WorkspaceArchiveResult::refused(problems),
    };

    // The store checks the files' format before the target is touched. The check runs in a temporary folder that is
    // removed afterwards.
    let staging = std::env::temp_dir().join(format!("ricochet-archive-{}", new_id()));
    let checked = check_in_staging(&staging, &files);
    let _ = fs::remove_dir_all(&staging);
    match checked {
        Err(error) => return WorkspaceArchiveResult::failed(format!("The archive could not be imported: {error}")),
        Ok(Err(problems)) => return WorkspaceArchiveResult::refused(problems),
        Ok(Ok(())) => {}
    }

    let created = !target.exists();
    match write_files(&target, &files) {
        Ok(()) => WorkspaceArchiveResult::ok(files.len()),
        Err(error) => {
            remove_what_was_written(&target, created);
            WorkspaceArchiveResult::failed(format!("The archive could not be imported: {error}"))
        }
    }
}

/// Writes the entries into a temporary folder and loads them with the store. Returns the problems the store found,
/// or the I/O error when the temporary folder could not be written. The inner result is the outcome of the check.
fn check_in_staging(staging: &Path, files: &[(String, Vec<u8>)]) -> Result<Result<(), Vec<String>>, io::Error> {
    for (name, bytes) in files {
        let path = name_to_path(staging, name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, bytes)?;
    }

    let mut folder = WorkspaceFolder::new(staging);
    let (workspace, issues) = folder.load()?;
    let mut problems: Vec<String> = issues.iter().map(issue_text).collect();
    problems.extend(secret_problems(&workspace));
    Ok(if problems.is_empty() { Ok(()) } else { Err(problems) })
}

/// Reads and checks every entry. Returns the workspace files, or every problem found.
fn read_entries(zip_path: &Path, limits: ArchiveLimits) -> Result<Vec<(String, Vec<u8>)>, Vec<String>> {
    let file = File::open(zip_path).map_err(|error| vec![format!("The archive could not be read: {error}")])?;
    let mut archive = ZipArchive::new(BufReader::new(file)).map_err(|error| vec![format!("The archive could not be read: {error}")])?;
    if archive.len() > limits.max_entries {
        return Err(vec![format!("The archive holds more than {} entries, so it was refused as too large.", limits.max_entries)]);
    }

    let mut problems = Vec::new();
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    let mut total: u64 = 0;
    for index in 0..archive.len() {
        let name = match archive.by_index_raw(index).map(|entry| entry.name().map(|name| name.to_string())) {
            Ok(Ok(name)) => name,
            Ok(Err(error)) | Err(error) => {
                problems.push(format!("The archive could not be read: {error}"));
                continue;
            }
        };

        if let Some(folder) = name.strip_suffix('/') {
            // A folder entry. It creates nothing by itself, but still must not point outside the archive.
            if !is_safe_name(folder) {
                problems.push(format!("{name}: The path is not allowed."));
            }
            continue;
        }

        if !is_safe_name(&name) {
            problems.push(format!("{name}: The path is not allowed. Entries must be relative, use forward slashes, and stay inside the archive."));
            continue;
        }

        let Some(kind) = kind_of(&name) else {
            problems.push(format!("{name}: This is not a workspace file, so it was refused."));
            continue;
        };

        if !seen.insert(name.clone()) {
            problems.push(format!("{name}: The archive holds this file more than once."));
            continue;
        }

        let bytes = match read_capped(&mut archive, index, limits.max_entry_bytes) {
            Ok(bytes) => bytes,
            Err(problem) => {
                problems.push(format!("{name}: {problem}"));
                continue;
            }
        };

        total += bytes.len() as u64;
        if total > limits.max_total_bytes {
            return Err(vec![format!(
                "The archive unpacks to more than {} bytes, so it was refused as too large.",
                limits.max_total_bytes
            )]);
        }

        let Ok(text) = std::str::from_utf8(&bytes) else {
            problems.push(format!("{name}: It is not UTF-8 text."));
            continue;
        };

        if let Some(problem) = shape_problem(kind, text) {
            problems.push(format!("{name}: {problem}"));
            continue;
        }

        files.push((name, bytes));
    }

    if !seen.contains(MARKER_FILE_NAME) {
        problems.push(format!("The archive has no {MARKER_FILE_NAME}, so it is not a workspace."));
    }

    if problems.is_empty() { Ok(files) } else { Err(problems) }
}

/// The fields of a Request or Folder file that the archive checks itself, as C# `Check` does. The store
/// skips a Request inside a folder it does not read, so it would not notice a file that is wrong on its own.
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct ItemShape {
    id: Option<String>,
    request: Option<ApiRequest>,
}

/// Why a Request or Folder file cannot be used, or None. Other kinds are checked by the store when it loads them.
fn shape_problem(kind: Kind, text: &str) -> Option<String> {
    if !matches!(kind, Kind::Request | Kind::Folder) {
        return None;
    }
    if text.trim() == "null" {
        return Some("The file is empty.".to_string());
    }

    let shape = match serde_json::from_str::<ItemShape>(text) {
        Ok(shape) => shape,
        Err(error) => return Some(format!("It is not readable JSON in the workspace format: {error}")),
    };
    if shape.id.as_deref().is_none_or(|id| id.trim().is_empty()) {
        return Some("It has no id.".to_string());
    }
    if kind == Kind::Request && shape.request.is_none() {
        return Some("It has no request.".to_string());
    }
    None
}

/// Reads one entry, stopping as soon as it unpacks past `max` bytes.
fn read_capped<R: Read + Seek>(archive: &mut ZipArchive<R>, index: usize, max: u64) -> Result<Vec<u8>, String> {
    let entry = archive.by_index(index).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    entry.take(max + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > max {
        return Err(format!("it unpacks to more than {max} bytes, so it was refused as too large."));
    }
    Ok(bytes)
}

/// Writes the archive's files and the same `.gitignore` a new workspace gets (ADR 0031). The folder is created here.
fn write_files(target: &Path, files: &[(String, Vec<u8>)]) -> io::Result<()> {
    fs::create_dir_all(target)?;
    for (name, bytes) in files {
        let path = name_to_path(target, name);
        if !path.starts_with(target) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("{name} would be written outside the new folder.")));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, bytes)?;
    }
    fs::write(target.join(GITIGNORE_FILE_NAME), GITIGNORE_CONTENT)
}

/// Writes a ZIP to a temporary file, then moves it over `zip_path`, so a failed write leaves no partial archive.
fn write_zip(zip_path: &Path, files: &[(String, Vec<u8>)]) -> io::Result<()> {
    let temp = PathBuf::from(format!("{}.tmp", zip_path.display()));
    let result = (|| -> io::Result<()> {
        let file = File::create(&temp)?;
        let mut zip = ZipWriter::new(BufWriter::new(file));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, bytes) in files {
            zip.start_file(name.as_str(), options).map_err(io::Error::other)?;
            zip.write_all(bytes)?;
        }
        zip.finish().map_err(io::Error::other)?.flush()?;
        fs::rename(&temp, zip_path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Every file that could be shared, as a relative path with forward slashes. `kind_of` decides which are kept.
fn shared_candidates(root: &Path) -> Vec<String> {
    let mut names = Vec::new();
    for name in [MARKER_FILE_NAME, GLOBALS_FILE_NAME] {
        if root.join(name).is_file() {
            names.push(name.to_string());
        }
    }
    for dir in [ENVIRONMENTS_DIR, COLLECTIONS_DIR] {
        collect_files(root, dir, &mut names);
    }
    names
}

fn collect_files(root: &Path, relative: &str, names: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(name_to_path(root, relative)) else {
        return;
    };
    for entry in entries.flatten() {
        let child = format!("{relative}/{}", entry.file_name().to_string_lossy());
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => collect_files(root, &child, names),
            Ok(kind) if kind.is_file() && child.to_ascii_lowercase().ends_with(".json") => names.push(child),
            _ => {}
        }
    }
}

/// Which shareable file a relative path is, or None when it is not one (including personal files).
fn kind_of(name: &str) -> Option<Kind> {
    let parts: Vec<&str> = name.split('/').collect();
    let leaf = *parts.last()?;
    if !leaf.to_ascii_lowercase().ends_with(".json") || PERSONAL_FILE_NAMES.iter().any(|personal| personal.eq_ignore_ascii_case(leaf)) {
        return None;
    }

    if parts.len() == 1 {
        return match leaf {
            MARKER_FILE_NAME => Some(Kind::Marker),
            GLOBALS_FILE_NAME => Some(Kind::Globals),
            _ => None,
        };
    }

    if parts.len() == 2 && parts[0] == ENVIRONMENTS_DIR {
        return Some(Kind::Environment);
    }

    if parts[0] != COLLECTIONS_DIR || parts.len() < 3 {
        return None;
    }

    match leaf {
        COLLECTION_FILE_NAME if parts.len() == 3 => Some(Kind::Collection),
        FOLDER_FILE_NAME if parts.len() >= 4 => Some(Kind::Folder),
        COLLECTION_FILE_NAME | FOLDER_FILE_NAME => None,
        _ => Some(Kind::Request),
    }
}

/// False for an empty name, an absolute or rooted path, a drive letter, a backslash, a NUL, or any "." or ".." part,
/// so an entry can never point outside the folder it is written into.
fn is_safe_name(name: &str) -> bool {
    if name.is_empty() || name.starts_with('/') || name.contains(['\\', ':', '\0']) {
        return false;
    }
    name.split('/').all(|part| !matches!(part, "" | "." | ".."))
}

/// The relative name as a path under `root`. Callers pass only names that `is_safe_name` accepts or that this module built.
fn name_to_path(root: &Path, name: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    for part in name.split('/').filter(|part| !part.is_empty()) {
        path.push(part);
    }
    debug_assert!(path.components().all(|component| !matches!(component, Component::ParentDir)));
    path
}

fn issue_text(issue: &WorkspaceIssue) -> String {
    format!("{}: {}", issue.path, issue.problem)
}

/// Secret values (a KeyValue with `is_secret` set and a value) are never allowed into a shared file or an archive.
/// The value itself is never named in the problem.
fn secret_problems(workspace: &Workspace) -> Vec<String> {
    let mut problems = Vec::new();
    let mut check = |owner: &str, values: &[KeyValue]| {
        for value in values.iter().filter(|value| value.is_secret && !value.value.is_empty()) {
            problems.push(format!(
                "{owner}: the Secret {} holds a value in a shared file. Secret values never go into an archive.",
                value.key
            ));
        }
    };

    check(GLOBALS_FILE_NAME, &workspace.globals);
    for environment in &workspace.environments {
        check(&format!("{ENVIRONMENTS_DIR}/{}", environment.name), &environment.variables);
    }
    for collection in &workspace.collections {
        check_collection(collection, &mut check);
    }
    problems
}

fn check_collection(collection: &Collection, check: &mut impl FnMut(&str, &[KeyValue])) {
    let owner = format!("{COLLECTIONS_DIR}/{}", collection.name);
    check(&owner, &collection.variables);
    for item in &collection.items {
        check_item(&owner, item, check);
    }
}

fn check_item(parent: &str, item: &CollectionItem, check: &mut impl FnMut(&str, &[KeyValue])) {
    let owner = format!("{parent}/{}", item.name);
    check(&owner, &item.variables);
    if let Some(request) = &item.request {
        check(&owner, &request.headers);
    }
    for child in &item.children {
        check_item(&owner, child, check);
    }
}

fn remove_what_was_written(target: &Path, created: bool) {
    // Failures here are ignored: the error reported is the one that matters.
    if created {
        let _ = fs::remove_dir_all(target);
        return;
    }

    // The folder was empty before the import, so removing everything in it restores that.
    if let Ok(entries) = fs::read_dir(target) {
        for entry in entries.flatten() {
            let path = entry.path();
            let _ = if path.is_dir() { fs::remove_dir_all(&path) } else { fs::remove_file(&path) };
        }
    }
}
