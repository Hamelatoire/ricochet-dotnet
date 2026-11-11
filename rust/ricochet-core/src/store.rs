//! Port of `WorkspaceFolder.cs` and the shared and personal parts of `WorkspaceStore.cs` (ADR 0003, ADR 0034).
//!
//! A Workspace is a folder with one JSON file per Request, so that git diffs and merges stay readable. Personal
//! state (`state.json`, `history.json`) lives apart in the personal directory and is never shared.
//!
//! Safety rules, as in C#: a save changes or deletes only files this module read successfully or wrote itself.
//! A file that fails to parse (for example, one with merge conflict markers) or is not recognised is reported and
//! left exactly as it is.
//!
//! Not ported yet: watching the folder for outside changes, the Secret split (`WorkspaceSecrets`), opening another
//! folder (`OpenFolder`), the old app's plain-text copies, and the personal Settings, theme, window and tab fields.
//! Those fields are kept as raw JSON in `state.json`, so a save never drops them.
//!
//! Secrets: this module writes what it is given. Until the Secret split is ported, callers must not pass a Workspace
//! that holds Secret values, because they would be written to the shared folder as plain text.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::models::{ApiEnvironment, ApiRequest, Collection, CollectionItem, HistoryEntry, KeyValue, StoredScript, Workspace, new_id};

pub const MARKER_FILE_NAME: &str = "ricochet.json";
/// Written once, when a Workspace folder is first saved, so personal files are not committed by accident (ADR 0031).
pub const GITIGNORE_FILE_NAME: &str = ".gitignore";
pub const GITIGNORE_CONTENT: &str = concat!(
    "# Written by Ricochet when this workspace was created. Edit it as you like; Ricochet won't overwrite it.\n",
    "# Temporary files left by an interrupted save\n",
    "*.tmp\n",
    "# Personal and secret-holding files, in case they end up in this folder\n",
    "state.json\n",
    "history.json\n",
    "secrets.dat\n",
    ".env\n",
    "*.secret\n",
);

const FORMAT: u32 = 1;
const COLLECTION_FILE_NAME: &str = "collection.json";
const FOLDER_FILE_NAME: &str = "folder.json";
const GLOBALS_FILE_NAME: &str = "globals.json";
const COLLECTIONS_DIR: &str = "collections";
const ENVIRONMENTS_DIR: &str = "environments";
const STATE_FILE_NAME: &str = "state.json";
const HISTORY_FILE_NAME: &str = "history.json";
pub(crate) const LEGACY_FILE_NAME: &str = "workspace.json";
pub(crate) const LEGACY_BACKUP_SUFFIX: &str = ".bak";
const DEFAULT_FOLDER_NAME: &str = "workspace";

/// A file that could not be used. It stays on disk untouched until someone fixes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceIssue {
    pub path: String,
    pub problem: String,
}

impl WorkspaceIssue {
    fn new(path: impl Into<String>, problem: impl Into<String>) -> Self {
        Self { path: path.into(), problem: problem.into() }
    }
}

// On-disk shapes. Every field is optional so that a hand-edited file with explicit nulls still loads, as in C#.
// Writing sets the fields C# writes; an unset optional field is left out of the file.

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct MarkerFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    order: Option<Vec<String>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct CollectionFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    variables: Option<Vec<KeyValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scripts: Option<Vec<StoredScript>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    order: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    spec_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hidden_item_ids: Option<Vec<String>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct FolderFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    /// Left out when there are none, so a Folder without Variables keeps its file unchanged (ADR 0020).
    #[serde(skip_serializing_if = "Option::is_none")]
    variables: Option<Vec<KeyValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scripts: Option<Vec<StoredScript>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    order: Option<Vec<String>>,
    /// Left out when there are none (ADR 0030).
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<Vec<String>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RequestFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scripts: Option<Vec<StoredScript>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    request: Option<ApiRequest>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct EnvironmentFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    variables: Option<Vec<KeyValue>>,
}

/// Personal state in `state.json`. Fields this port does not read are kept as raw JSON, so a save never drops them.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct PersonalState {
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace_folder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    active_environment_id: Option<String>,
    #[serde(flatten)]
    other: Map<String, Value>,
}

/// Pretty JSON with LF newlines and a final newline, as C# writes shared files.
fn to_json<T: Serialize + ?Sized>(value: &T) -> String {
    let mut text = serde_json::to_string_pretty(value).expect("workspace data always serializes");
    text.push('\n');
    text
}

fn is_json(name: &str) -> bool {
    Path::new(name).extension().is_some_and(|extension| extension == "json")
}

fn file_stem(name: &str) -> String {
    Path::new(name).file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default()
}

fn join(parent: &str, leaf: &str) -> String {
    if parent.is_empty() { leaf.to_string() } else { format!("{parent}/{leaf}") }
}

/// Names sort ignoring case, as C# `OrdinalIgnoreCase` does.
fn by_name(a: &str, b: &str) -> Ordering {
    a.to_uppercase().cmp(&b.to_uppercase())
}

/// Drops explicit nulls at the top level and inside `request`, so that a missing value takes its default, as C# does.
fn drop_nulls(value: &mut Value) {
    if let Value::Object(map) = value {
        map.retain(|_, field| !field.is_null());
        if let Some(Value::Object(request)) = map.get_mut("request") {
            request.retain(|_, field| !field.is_null());
        }
    }
}

/// Trims each tag, drops blanks and repeats (ignoring case), and keeps the first spelling. None when none are left.
pub fn normalize_tags(tags: Option<&[String]>) -> Option<Vec<String>> {
    let mut result: Vec<String> = Vec::new();
    for tag in tags?.iter() {
        let trimmed = tag.trim();
        if !trimmed.is_empty() && !result.iter().any(|kept| kept.eq_ignore_ascii_case(trimmed)) {
            result.push(trimmed.to_string());
        }
    }
    (!result.is_empty()).then_some(result)
}

/// The readable part of a file name, followed by part of the id so that equal names can't clash.
pub fn slug(name: &str, id: &str) -> String {
    const MAX_NAME_LENGTH: usize = 40;
    const ID_LENGTH: usize = 8;

    let mut text = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            text.push(c);
        } else if !text.is_empty() && !text.ends_with('-') {
            text.push('-');
        }

        if text.len() >= MAX_NAME_LENGTH {
            break;
        }
    }

    let text = text.trim_matches('-');
    let suffix: String = id.chars().filter(|c| c.is_ascii_alphanumeric()).take(ID_LENGTH).collect();
    if text.is_empty() { format!("item-{suffix}") } else { format!("{text}-{suffix}") }
}

/// Makes the file under a Request or Folder usable, or reports why not.
fn normalize_request(mut request: ApiRequest) -> ApiRequest {
    if request.id.is_empty() {
        request.id = new_id();
    }
    if request.method.is_empty() {
        request.method = "GET".to_string();
    }
    request
}

/// Mirrors `WorkspaceFolder.LooksLikeWorkspace`: the marker file exists, or the collections folder does.
pub fn looks_like_workspace(root: &Path) -> bool {
    root.join(MARKER_FILE_NAME).is_file() || root.join("collections").is_dir()
}

/// The shareable part of a Workspace, as one file per item. See docs/adr/0003-git-friendly-workspace-files.md.
#[derive(Debug)]
pub struct WorkspaceFolder {
    root: PathBuf,
    id: String,
    /// What each understood file means, in the form we would write it. A save writes only what differs.
    understood: BTreeMap<String, String>,
    /// Every file as last seen, including unreadable ones, so outside changes can be noticed.
    seen: BTreeMap<String, String>,
    /// The file name each item already has, as (leaf, name). It changes only when the item is renamed in the app.
    leaves: HashMap<String, (String, String)>,
}

impl WorkspaceFolder {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into(), id: String::new(), understood: BTreeMap::new(), seen: BTreeMap::new(), leaves: HashMap::new() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Identifies the Workspace. Set by `load`, kept in `ricochet.json`.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Reads the folder. Unreadable and unrecognised files are reported, and nothing in them is changed by a save.
    pub fn load(&mut self) -> io::Result<(Workspace, Vec<WorkspaceIssue>)> {
        self.understood.clear();
        self.leaves.clear();
        let mut issues = Vec::new();
        fs::create_dir_all(&self.root)?;
        self.seen = self.scan(&mut issues);

        let marker: Option<MarkerFile> = self.read_file(MARKER_FILE_NAME, &mut issues);
        self.id = match marker.as_ref().and_then(|m| m.id.clone()) {
            Some(id) if !id.is_empty() => id,
            _ => new_id(),
        };
        let order = marker.and_then(|m| m.order).unwrap_or_default();

        let workspace = Workspace {
            collections: self.read_collections(&order, &mut issues),
            environments: self.read_environments(&mut issues),
            globals: self.read_globals(&mut issues),
            ..Workspace::default()
        };
        Ok((workspace, issues))
    }

    /// Writes what changed and removes what was deleted. Returns the files left alone because they hold content this
    /// folder could not read, such as a file with merge conflict markers at a path a new item would use.
    pub fn save(&mut self, workspace: &Workspace) -> io::Result<Vec<WorkspaceIssue>> {
        let is_new = !self.full(MARKER_FILE_NAME).exists();
        let mut desired = self.render(workspace);
        self.add_gitignore(&mut desired, is_new);

        let mut left_alone = Vec::new();
        for (path, content) in &desired {
            if self.understood.get(path) == Some(content) {
                continue;
            }
            if self.seen.contains_key(path) && !self.understood.contains_key(path) {
                left_alone.push(WorkspaceIssue::new(path.as_str(), "The file could not be read, so it was left as it is."));
                continue;
            }

            write_atomically(&self.full(path), content)?;
            self.understood.insert(path.clone(), content.clone());

            // The scan doesn't read .gitignore, so writing it is not an outside change.
            if path != GITIGNORE_FILE_NAME {
                self.seen.insert(path.clone(), content.clone());
            }
        }

        let stale: Vec<String> = self.understood.keys().filter(|path| !desired.contains_key(*path)).cloned().collect();
        for path in stale {
            remove_if_exists(&self.full(&path))?;
            self.understood.remove(&path);
            self.seen.remove(&path);
            self.remove_empty_parents(&path)?;
        }

        Ok(left_alone)
    }

    /// True when files differ from what was last read or written, for example after a `git pull`.
    pub fn has_outside_changes(&self) -> bool {
        let current = self.scan(&mut Vec::new());
        current.len() != self.seen.len() || current.iter().any(|(path, text)| self.seen.get(path) != Some(text))
    }

    /// The `.gitignore` Ricochet wrote is kept, with the user's edits. One is written only for a new Workspace.
    fn add_gitignore(&self, desired: &mut BTreeMap<String, String>, is_new: bool) {
        if let Some(written) = self.understood.get(GITIGNORE_FILE_NAME) {
            desired.insert(GITIGNORE_FILE_NAME.to_string(), written.clone());
        } else if is_new && !self.full(GITIGNORE_FILE_NAME).exists() {
            desired.insert(GITIGNORE_FILE_NAME.to_string(), GITIGNORE_CONTENT.to_string());
        }
    }

    fn full(&self, relative: &str) -> PathBuf {
        relative.split('/').filter(|part| !part.is_empty()).fold(self.root.clone(), |path, part| path.join(part))
    }

    /// The entries directly inside a folder, as (files, directories), each sorted ordinally. Missing folders are empty.
    fn children_of(&self, relative: &str) -> (Vec<String>, Vec<String>) {
        let mut files = Vec::new();
        let mut dirs = Vec::new();
        if let Ok(entries) = fs::read_dir(self.full(relative)) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                match entry.file_type() {
                    Ok(kind) if kind.is_dir() => dirs.push(name),
                    Ok(kind) if kind.is_file() => files.push(name),
                    _ => {}
                }
            }
        }
        files.sort();
        dirs.sort();
        (files, dirs)
    }

    fn has_json_below(&self, relative: &str) -> bool {
        let (files, dirs) = self.children_of(relative);
        files.iter().any(|file| is_json(file)) || dirs.iter().any(|dir| self.has_json_below(&join(relative, dir)))
    }

    fn scan(&self, issues: &mut Vec<WorkspaceIssue>) -> BTreeMap<String, String> {
        let mut found = BTreeMap::new();
        for name in [MARKER_FILE_NAME, GLOBALS_FILE_NAME] {
            if self.full(name).is_file() {
                self.scan_file(name, &mut found, issues);
            }
        }
        for dir in [COLLECTIONS_DIR, ENVIRONMENTS_DIR] {
            self.scan_json_tree(dir, &mut found, issues);
        }
        found
    }

    fn scan_json_tree(&self, relative: &str, found: &mut BTreeMap<String, String>, issues: &mut Vec<WorkspaceIssue>) {
        let (files, dirs) = self.children_of(relative);
        for file in files.iter().filter(|file| is_json(file)) {
            self.scan_file(&join(relative, file), found, issues);
        }
        for dir in dirs {
            self.scan_json_tree(&join(relative, &dir), found, issues);
        }
    }

    fn scan_file(&self, relative: &str, found: &mut BTreeMap<String, String>, issues: &mut Vec<WorkspaceIssue>) {
        match fs::read(self.full(relative)) {
            Ok(bytes) => {
                found.insert(relative.to_string(), String::from_utf8_lossy(&bytes).replace("\r\n", "\n"));
            }
            Err(error) => issues.push(WorkspaceIssue::new(relative, error.to_string())),
        }
    }

    /// Parses a file that was scanned. A file that is missing, unreadable or invalid is reported and not understood.
    fn read_file<T>(&mut self, relative: &str, issues: &mut Vec<WorkspaceIssue>) -> Option<T>
    where
        T: Serialize + DeserializeOwned,
    {
        let text = self.seen.get(relative)?.clone();
        let mut value: Value = match serde_json::from_str(&text) {
            Ok(value) => value,
            Err(error) => {
                issues.push(WorkspaceIssue::new(relative, error.to_string()));
                return None;
            }
        };
        if value.is_null() {
            issues.push(WorkspaceIssue::new(relative, "The file is empty."));
            return None;
        }

        drop_nulls(&mut value);
        match serde_json::from_value::<T>(value) {
            Ok(file) => {
                self.understood.insert(relative.to_string(), to_json(&file));
                Some(file)
            }
            Err(error) => {
                issues.push(WorkspaceIssue::new(relative, error.to_string()));
                None
            }
        }
    }

    /// Checks an id. A file with no id, or a repeat of one, is reported and not understood.
    fn usable_id(&mut self, id: Option<&str>, relative: &str, ids: &mut HashSet<String>, issues: &mut Vec<WorkspaceIssue>) -> Option<String> {
        let Some(id) = id.filter(|id| !id.trim().is_empty()) else {
            self.forget(relative);
            issues.push(WorkspaceIssue::new(relative, "It has no id."));
            return None;
        };

        if ids.insert(id.to_string()) {
            Some(id.to_string())
        } else {
            self.forget(relative);
            issues.push(WorkspaceIssue::new(relative, format!("Another item already uses the id {id}.")));
            None
        }
    }

    /// Anything not taken into the model must never count as ours to delete.
    fn forget(&mut self, relative: &str) {
        self.understood.remove(relative);
    }

    fn read_collections(&mut self, order: &[String], issues: &mut Vec<WorkspaceIssue>) -> Vec<Collection> {
        let mut collections = Vec::new();
        let mut ids = HashSet::new();
        let (_, dirs) = self.children_of(COLLECTIONS_DIR);

        for dir_name in dirs {
            let relative = join(COLLECTIONS_DIR, &dir_name);
            let file_path = join(&relative, COLLECTION_FILE_NAME);
            let Some(file) = self.read_file::<CollectionFile>(&file_path, issues) else { continue };
            let Some(id) = self.usable_id(file.id.as_deref(), &file_path, &mut ids, issues) else { continue };

            let name = file.name.unwrap_or_default();
            self.leaves.insert(id.clone(), (dir_name, name.clone()));
            let items = self.read_children(&relative, &file.order.unwrap_or_default(), issues);
            collections.push(Collection {
                id,
                name,
                items,
                variables: file.variables.unwrap_or_default(),
                scripts: file.scripts.unwrap_or_default(),
                spec_url: file.spec_url,
                hidden_item_ids: file.hidden_item_ids,
            });
        }

        // Follow the saved order. A Collection the order doesn't list goes last, by name, as a merge can add one.
        let rank: HashMap<&str, usize> = order.iter().enumerate().rev().map(|(index, id)| (id.as_str(), index)).collect();
        let rank_of = |collection: &Collection| rank.get(collection.id.as_str()).copied().unwrap_or(usize::MAX);
        collections.sort_by(|a, b| rank_of(a).cmp(&rank_of(b)).then_with(|| by_name(&a.name, &b.name)));
        collections
    }

    fn read_children(&mut self, relative_dir: &str, order: &[String], issues: &mut Vec<WorkspaceIssue>) -> Vec<CollectionItem> {
        let mut children: Vec<CollectionItem> = Vec::new();
        let mut ids = HashSet::new();
        let (files, dirs) = self.children_of(relative_dir);

        for file_name in files {
            if !is_json(&file_name) || file_name == COLLECTION_FILE_NAME || file_name == FOLDER_FILE_NAME {
                continue;
            }

            let relative = join(relative_dir, &file_name);
            let Some(file) = self.read_file::<RequestFile>(&relative, issues) else { continue };
            let Some(id) = self.usable_id(file.id.as_deref(), &relative, &mut ids, issues) else { continue };

            let Some(request) = file.request else {
                self.forget(&relative);
                issues.push(WorkspaceIssue::new(relative, "It has no request."));
                continue;
            };

            let name = file.name.unwrap_or_default();
            self.leaves.insert(id.clone(), (file_stem(&file_name), name.clone()));
            children.push(CollectionItem {
                id,
                name,
                request: Some(normalize_request(request)),
                scripts: file.scripts.unwrap_or_default(),
                ..CollectionItem::default()
            });
        }

        for dir_name in dirs {
            let relative = join(relative_dir, &dir_name);
            let folder_path = join(&relative, FOLDER_FILE_NAME);
            if !self.full(&folder_path).is_file() {
                if self.has_json_below(&relative) {
                    issues.push(WorkspaceIssue::new(relative, format!("This folder has no {FOLDER_FILE_NAME}, so its contents were not loaded.")));
                }
                continue;
            }

            let Some(file) = self.read_file::<FolderFile>(&folder_path, issues) else { continue };
            let Some(id) = self.usable_id(file.id.as_deref(), &folder_path, &mut ids, issues) else { continue };

            let name = file.name.unwrap_or_default();
            self.leaves.insert(id.clone(), (dir_name, name.clone()));
            let folder_children = self.read_children(&relative, &file.order.clone().unwrap_or_default(), issues);
            children.push(CollectionItem {
                id,
                name,
                variables: file.variables.unwrap_or_default(),
                scripts: file.scripts.unwrap_or_default(),
                tags: normalize_tags(file.tags.as_deref()),
                children: folder_children,
                ..CollectionItem::default()
            });
        }

        // Follow the saved order. A merge can add items it doesn't list, which then go last, by name.
        let mut result = Vec::new();
        for id in order {
            match children.iter().position(|child| &child.id == id) {
                Some(position) => result.push(children.remove(position)),
                None => issues.push(WorkspaceIssue::new(relative_dir, format!("The order lists an item that was not found: {id}"))),
            }
        }
        children.sort_by(|a, b| by_name(&a.name, &b.name));
        result.extend(children);
        result
    }

    fn read_environments(&mut self, issues: &mut Vec<WorkspaceIssue>) -> Vec<ApiEnvironment> {
        let mut environments = Vec::new();
        let mut ids = HashSet::new();
        let (files, _) = self.children_of(ENVIRONMENTS_DIR);

        for file_name in files.into_iter().filter(|file| is_json(file)) {
            let relative = join(ENVIRONMENTS_DIR, &file_name);
            let Some(file) = self.read_file::<EnvironmentFile>(&relative, issues) else { continue };
            let Some(id) = self.usable_id(file.id.as_deref(), &relative, &mut ids, issues) else { continue };

            let name = file.name.unwrap_or_default();
            self.leaves.insert(id.clone(), (file_stem(&file_name), name.clone()));
            environments.push(ApiEnvironment { id, name, variables: file.variables.unwrap_or_default() });
        }

        environments.sort_by(|a, b| by_name(&a.name, &b.name));
        environments
    }

    fn read_globals(&mut self, issues: &mut Vec<WorkspaceIssue>) -> Vec<KeyValue> {
        self.read_file::<Vec<KeyValue>>(GLOBALS_FILE_NAME, issues).unwrap_or_default()
    }

    /// The files a Workspace should have on disk, by relative path.
    fn render(&mut self, workspace: &Workspace) -> BTreeMap<String, String> {
        let mut files = BTreeMap::new();

        files.insert(
            MARKER_FILE_NAME.to_string(),
            to_json(&MarkerFile {
                id: Some(self.id.clone()),
                format: Some(FORMAT),
                order: Some(workspace.collections.iter().map(|c| c.id.clone()).collect()),
            }),
        );

        for collection in &workspace.collections {
            let dir = self.path_for(&files, COLLECTIONS_DIR, &collection.name, &collection.id, "");
            files.insert(
                format!("{dir}/{COLLECTION_FILE_NAME}"),
                to_json(&CollectionFile {
                    id: Some(collection.id.clone()),
                    name: Some(collection.name.clone()),
                    variables: Some(collection.variables.clone()),
                    scripts: Some(collection.scripts.clone()),
                    order: Some(collection.items.iter().map(|i| i.id.clone()).collect()),
                    spec_url: collection.spec_url.clone(),
                    hidden_item_ids: collection.hidden_item_ids.clone(),
                }),
            );
            for item in &collection.items {
                self.add_item(&mut files, item, &dir);
            }
        }

        for environment in &workspace.environments {
            let path = self.path_for(&files, ENVIRONMENTS_DIR, &environment.name, &environment.id, ".json");
            files.insert(path, to_json(environment));
        }

        if !workspace.globals.is_empty() || self.understood.contains_key(GLOBALS_FILE_NAME) {
            files.insert(GLOBALS_FILE_NAME.to_string(), to_json(&workspace.globals));
        }

        files
    }

    fn add_item(&mut self, files: &mut BTreeMap<String, String>, item: &CollectionItem, parent: &str) {
        if let Some(request) = &item.request {
            let path = self.path_for(files, parent, &item.name, &item.id, ".json");
            files.insert(
                path,
                to_json(&RequestFile {
                    id: Some(item.id.clone()),
                    name: Some(item.name.clone()),
                    scripts: Some(item.scripts.clone()),
                    request: Some(request.clone()),
                }),
            );
            return;
        }

        let dir = self.path_for(files, parent, &item.name, &item.id, "");
        files.insert(
            format!("{dir}/{FOLDER_FILE_NAME}"),
            to_json(&FolderFile {
                id: Some(item.id.clone()),
                name: Some(item.name.clone()),
                variables: (!item.variables.is_empty()).then(|| item.variables.clone()),
                scripts: Some(item.scripts.clone()),
                order: Some(item.children.iter().map(|c| c.id.clone()).collect()),
                tags: normalize_tags(item.tags.as_deref()),
            }),
        );
        for child in &item.children {
            self.add_item(files, child, &dir);
        }
    }

    /// The path an item is written to. It keeps its file name until it is renamed, and gets its id appended when
    /// another item, or an unreadable file, already holds the name.
    fn path_for(&mut self, files: &BTreeMap<String, String>, parent: &str, name: &str, id: &str, extension: &str) -> String {
        let is_directory = extension.is_empty();
        let keeps_location = self.leaves.get(id).is_some_and(|(_, known_name)| known_name.as_str() == name);
        let mut leaf = if keeps_location { self.leaves[id].0.clone() } else { slug(name, id) };

        let candidate = format!("{parent}/{leaf}{extension}");
        if self.used_by_another_item(files, &candidate, is_directory) || (!keeps_location && self.holds_unreadable_file(&candidate, is_directory)) {
            leaf = format!("{leaf}-{id}");
        }

        self.leaves.insert(id.to_string(), (leaf.clone(), name.to_string()));
        format!("{parent}/{leaf}{extension}")
    }

    fn used_by_another_item(&self, files: &BTreeMap<String, String>, candidate: &str, is_directory: bool) -> bool {
        if is_directory {
            let prefix = format!("{candidate}/");
            files.keys().any(|key| key.starts_with(&prefix))
        } else {
            files.contains_key(candidate)
        }
    }

    /// A new location must also avoid anything already on disk that this folder could not read.
    fn holds_unreadable_file(&self, candidate: &str, is_directory: bool) -> bool {
        let blocks = |path: &str| !self.understood.contains_key(path);
        if is_directory {
            let prefix = format!("{candidate}/");
            self.seen.keys().any(|key| key.starts_with(&prefix) && blocks(key))
        } else {
            self.seen.contains_key(candidate) && blocks(candidate)
        }
    }

    fn remove_empty_parents(&self, relative: &str) -> io::Result<()> {
        let mut dir = self.full(relative).parent().map(Path::to_path_buf);
        while let Some(current) = dir {
            if current == self.root || !current.starts_with(&self.root) || !current.is_dir() {
                break;
            }
            let empty = match fs::read_dir(&current) {
                Ok(mut entries) => entries.next().is_none(),
                Err(_) => false,
            };
            if !empty {
                break;
            }
            fs::remove_dir(&current)?;
            dir = current.parent().map(Path::to_path_buf);
        }
        Ok(())
    }
}

fn write_atomically(path: &Path, content: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = PathBuf::from(format!("{}.tmp", path.display()));
    fs::write(&temp, content)?;
    fs::rename(&temp, path)
}

fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// The shared folder and personal files, kept apart (ADR 0003).
#[derive(Debug)]
pub struct WorkspaceStore {
    personal_dir: PathBuf,
    folder: Option<WorkspaceFolder>,
    legacy_pending: bool,
    state: PersonalState,
}

#[derive(Debug)]
pub struct WorkspaceLoadResult {
    pub workspace: Workspace,
    pub issues: Vec<WorkspaceIssue>,
    /// True when the files should be written, such as after a migration or on first start.
    pub needs_save: bool,
}

impl WorkspaceStore {
    pub fn new(personal_dir: impl Into<PathBuf>) -> Self {
        Self { personal_dir: personal_dir.into(), folder: None, legacy_pending: false, state: PersonalState::default() }
    }

    pub fn personal_dir(&self) -> &Path {
        &self.personal_dir
    }

    /// The shared folder, once loaded.
    pub fn folder_path(&self) -> Option<&Path> {
        self.folder.as_ref().map(WorkspaceFolder::root)
    }

    /// Identifies the Workspace once loaded, so that each one can keep its own Secrets (not yet ported).
    pub fn workspace_id(&self) -> Option<&str> {
        self.folder.as_ref().map(WorkspaceFolder::id)
    }

    pub fn load(&mut self) -> io::Result<WorkspaceLoadResult> {
        let mut issues = Vec::new();
        let state_exists = self.personal_path(STATE_FILE_NAME).exists();
        let state: PersonalState = self.read_personal(STATE_FILE_NAME, &mut issues).unwrap_or_default();
        self.state = state.clone();
        let history: Vec<HistoryEntry> = self.read_personal(HISTORY_FILE_NAME, &mut issues).unwrap_or_default();

        // A remembered folder that has gone (possibly an unplugged drive) is reported, and never recreated empty.
        let mut folder_path = self.default_folder_path();
        if let Some(remembered) = state.workspace_folder.as_deref().map(PathBuf::from) {
            if remembered.is_dir() {
                folder_path = remembered;
            } else {
                issues.push(WorkspaceIssue::new(
                    remembered.display().to_string(),
                    "This workspace folder was not found, so the default one is open instead.",
                ));
            }
        }

        let mut folder = WorkspaceFolder::new(&folder_path);
        let (shared, folder_issues) = folder.load()?;
        issues.extend(folder_issues);
        self.folder = Some(folder);

        let legacy_path = self.personal_path(LEGACY_FILE_NAME);
        if !state_exists && legacy_path.exists() {
            let legacy = load_legacy(&legacy_path)?;
            self.legacy_pending = legacy_path.exists();
            return Ok(WorkspaceLoadResult { workspace: legacy, issues, needs_save: true });
        }

        let workspace = Workspace { active_environment_id: state.active_environment_id.clone(), history, ..shared };
        Ok(WorkspaceLoadResult { workspace, issues, needs_save: !state_exists })
    }

    /// Writes the shared folder, then the personal files. The original single file is kept as a backup only after that.
    /// Returns the files the folder left alone (see [`WorkspaceFolder::save`]).
    pub fn save(&mut self, workspace: &Workspace) -> io::Result<Vec<WorkspaceIssue>> {
        let folder = self.folder.as_mut().ok_or_else(|| io::Error::other("Load the store first."))?;
        let left_alone = folder.save(workspace)?;

        self.write_personal(HISTORY_FILE_NAME, &workspace.history)?;
        let mut state = self.state.clone();
        state.workspace_folder = self.remembered_folder();
        state.active_environment_id = workspace.active_environment_id.clone();
        self.write_personal(STATE_FILE_NAME, &state)?;
        self.state = state;

        if self.legacy_pending {
            let legacy = self.personal_path(LEGACY_FILE_NAME);
            fs::rename(&legacy, format!("{}{LEGACY_BACKUP_SUFFIX}", legacy.display()))?;
            self.legacy_pending = false;
        }

        Ok(left_alone)
    }

    pub fn has_outside_changes(&self) -> bool {
        self.folder.as_ref().is_some_and(WorkspaceFolder::has_outside_changes)
    }

    /// Rereads the shared files. History and the active Environment are personal, so the caller keeps them.
    pub fn reload_shared(&mut self) -> io::Result<(Workspace, Vec<WorkspaceIssue>)> {
        let folder = self.folder.as_mut().ok_or_else(|| io::Error::other("Load the store first."))?;
        folder.load()
    }

    fn personal_path(&self, name: &str) -> PathBuf {
        self.personal_dir.join(name)
    }

    fn default_folder_path(&self) -> PathBuf {
        self.personal_dir.join(DEFAULT_FOLDER_NAME)
    }

    /// The default folder is not recorded, so moving the personal directory keeps working.
    fn remembered_folder(&self) -> Option<String> {
        let root = self.folder.as_ref()?.root();
        let default = self.default_folder_path();
        if root.to_string_lossy().eq_ignore_ascii_case(&default.to_string_lossy()) {
            None
        } else {
            Some(root.to_string_lossy().into_owned())
        }
    }

    /// Reads a personal file. One that cannot be parsed is set aside beside the original, and reported.
    fn read_personal<T: DeserializeOwned>(&self, name: &str, issues: &mut Vec<WorkspaceIssue>) -> Option<T> {
        let path = self.personal_path(name);
        if !path.exists() {
            return None;
        }

        let text = fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<Option<T>>(&text) {
            Ok(value) => value,
            Err(error) => {
                let kept = PathBuf::from(format!("{}.corrupt-{}", path.display(), chrono::Utc::now().format("%Y%m%d%H%M%S")));
                let _ = fs::rename(&path, &kept);
                issues.push(WorkspaceIssue::new(
                    kept.display().to_string(),
                    format!("This file could not be read, so it was set aside: {error}"),
                ));
                None
            }
        }
    }

    fn write_personal<T: Serialize + ?Sized>(&self, name: &str, value: &T) -> io::Result<()> {
        fs::create_dir_all(&self.personal_dir)?;
        let path = self.personal_path(name);
        let temp = PathBuf::from(format!("{}.tmp", path.display()));
        fs::write(&temp, serde_json::to_string_pretty(value).expect("personal data always serializes"))?;
        fs::rename(&temp, &path)
    }
}

/// Reads the original single-file format so it can be migrated. An unreadable file is kept beside the original.
fn load_legacy(path: &Path) -> io::Result<Workspace> {
    let text = fs::read_to_string(path)?;
    match serde_json::from_str::<Workspace>(&text) {
        Ok(workspace) => Ok(workspace.migrate_legacy_fields()),
        Err(_) => {
            let kept = PathBuf::from(format!("{}.corrupt-{}", path.display(), chrono::Utc::now().format("%Y%m%d%H%M%S")));
            fs::rename(path, kept)?;
            Ok(Workspace::default())
        }
    }
}
