//! Port of the personal part of `WorkspaceStore.cs` (ADR 0007, ADR 0043, ADR 0068): the reader for `state.json`.
//!
//! The file lives in the personal directory, which is app data, or `RICOCHET_DATA` when that is set. It is never
//! shared. Missing values take the defaults the C# records give, so an older or hand-trimmed file still loads.
//!
//! Failures, as in C#: a missing or unreadable file gives the defaults. A file that is not valid JSON, or has a
//! value of the wrong type, is moved beside the original as `state.json.corrupt-<timestamp>`, reported as an issue,
//! and the defaults are used. Saves (`save_active_environment`, `save_settings`) change only their own key.
//!
//! Not ported yet: the old app's copy of `workspace.json` that the store adopts on first run.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::send::SenderSettings;

use crate::key_value_columns::is_saveable;
use crate::store::WorkspaceIssue;

/// The personal state file name.
pub const STATE_FILE_NAME: &str = "state.json";

/// Personal settings for this person on this machine (ADR 0007). Defaults match the C# record.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PersonalSettings {
    pub request_timeout_seconds: i32,
    pub follow_redirects: bool,
    /// When off, any server certificate is accepted. The app warns while this is off.
    pub verify_tls_certificates: bool,
    pub max_response_megabytes: i32,
    pub wrap_response: bool,
    /// An HTTP proxy. Empty uses the operating system's proxy (ADR 0026).
    pub proxy_url: String,
    /// A PKCS #12 file holding a client certificate. Empty sends none. Only the path is stored.
    pub client_certificate_path: String,
}

impl Default for PersonalSettings {
    fn default() -> Self {
        Self {
            request_timeout_seconds: 120,
            follow_redirects: true,
            verify_tls_certificates: true,
            max_response_megabytes: 100,
            wrap_response: true,
            proxy_url: String::new(),
            client_certificate_path: String::new(),
        }
    }
}

impl PersonalSettings {
    /// The sender settings these give, as the C# `ApplySettings` passes them to `RequestSender.Configure`. A timeout
    /// or size limit below one, which a hand-edited file could hold, falls back to the default.
    pub fn sender_settings(&self) -> SenderSettings {
        let defaults = Self::default();
        let timeout = if self.request_timeout_seconds > 0 { self.request_timeout_seconds } else { defaults.request_timeout_seconds };
        let megabytes = if self.max_response_megabytes > 0 { self.max_response_megabytes } else { defaults.max_response_megabytes };
        SenderSettings {
            timeout: Duration::from_secs(u64::from(timeout.unsigned_abs())),
            follow_redirects: self.follow_redirects,
            verify_certificates: self.verify_tls_certificates,
            max_body_bytes: u64::from(megabytes.unsigned_abs()) * 1024 * 1024,
            proxy_url: self.proxy_url.clone(),
            client_certificate_path: self.client_certificate_path.clone(),
        }
    }
}

/// Where a bulk run stopped, so it can resume after the last id it processed.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BulkProgress {
    pub request_id: String,
    pub ids: String,
    pub last_id: Option<String>,
    pub processed: i32,
}

/// Where the main window was. Size and position are the normal (not maximised) bounds.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WindowPlacement {
    pub x: i32,
    pub y: i32,
    pub width: f64,
    pub height: f64,
    pub is_maximized: bool,
}

impl WindowPlacement {
    /// False for values no real window could have, such as a file edited by hand.
    pub fn is_reasonable(&self) -> bool {
        self.width.is_finite()
            && self.height.is_finite()
            && (200.0..=10000.0).contains(&self.width)
            && (150.0..=10000.0).contains(&self.height)
            && self.x > -20000
            && self.x < 20000
            && self.y > -20000
            && self.y < 20000
    }
}

/// The sidebar width and request pane height (ADR 0043). Each is optional, so one can be kept when the other is damaged.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PanelLayout {
    pub sidebar_width: Option<f64>,
    pub request_height: Option<f64>,
}

impl PanelLayout {
    /// The same layout with any value no real window could have dropped, so it falls back to the default.
    pub fn sanitized(&self) -> Self {
        Self {
            sidebar_width: self.sidebar_width.filter(|width| width.is_finite() && (120.0..=4000.0).contains(width)),
            request_height: self.request_height.filter(|height| height.is_finite() && (60.0..=4000.0).contains(height)),
        }
    }
}

/// Personal state in `state.json`. Missing fields take the defaults here.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PersonalState {
    pub workspace_folder: Option<String>,
    pub active_environment_id: Option<String>,
    /// "light" or "dark". None follows the operating system.
    pub theme: Option<String>,
    pub bulk: Vec<BulkProgress>,
    pub settings: PersonalSettings,
    pub window: Option<WindowPlacement>,
    /// The key column's share for each key-value table kind, such as "headers" (ADR 0074).
    pub key_value_splits: Option<BTreeMap<String, f64>>,
    pub panels: Option<PanelLayout>,
    /// The ids of the Requests open as tabs, in tab order (ADR 0041).
    pub open_tabs: Option<Vec<String>>,
    pub active_tab: Option<String>,
}

impl PersonalState {
    /// The saved window, or None when it is missing or not one a window could have (so the default is used).
    pub fn window(&self) -> Option<&WindowPlacement> {
        self.window.as_ref().filter(|window| window.is_reasonable())
    }

    /// The saved key column share for a table kind, or None when there is none or it is not a usable share.
    pub fn key_value_split(&self, kind: &str) -> Option<f64> {
        self.key_value_splits.as_ref()?.get(kind).copied().filter(|share| is_saveable(*share))
    }

    /// The saved panel layout with damaged values dropped, or None when nothing was saved.
    pub fn panels(&self) -> Option<PanelLayout> {
        self.panels.as_ref().map(PanelLayout::sanitized)
    }

    /// The open Request ids, or an empty list when none are open.
    pub fn open_tabs(&self) -> Vec<String> {
        self.open_tabs.clone().unwrap_or_default()
    }
}

/// The result of reading `state.json`: the state to use, and any file that had to be set aside.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PersonalStateLoad {
    pub state: PersonalState,
    pub issues: Vec<WorkspaceIssue>,
}

/// Reads `state.json` from the personal directory, using the rules described in the module docs.
pub fn load(personal_directory: &Path) -> PersonalStateLoad {
    let path = personal_directory.join(STATE_FILE_NAME);
    let Ok(text) = fs::read_to_string(&path) else {
        return PersonalStateLoad::default();
    };

    // A file holding only `null` gives the defaults, as the C# does.
    match serde_json::from_str::<Option<PersonalState>>(&text) {
        Ok(state) => PersonalStateLoad { state: state.unwrap_or_default(), issues: Vec::new() },
        Err(error) => {
            let kept = PathBuf::from(format!("{}.corrupt-{}", path.display(), chrono::Utc::now().format("%Y%m%d%H%M%S")));
            let issue = match fs::rename(&path, &kept) {
                Ok(()) => WorkspaceIssue {
                    path: kept.display().to_string(),
                    problem: format!("This file could not be read, so it was set aside: {error}"),
                },
                // It could not be moved, so it is left in place and still reported.
                Err(_) => WorkspaceIssue {
                    path: path.display().to_string(),
                    problem: format!("This file could not be read: {error}"),
                },
            };
            let issues = vec![issue];
            PersonalStateLoad { state: PersonalState::default(), issues }
        }
    }
}

/// The personal directory: `override_path` when it is set and not empty, otherwise `Ricochet` in the app data folder.
/// None when there is neither. Takes its inputs as values so it can be tested without changing the environment.
pub fn resolve_personal_directory(override_path: Option<&str>, app_data: Option<&Path>) -> Option<PathBuf> {
    match override_path {
        Some(path) if !path.is_empty() => Some(PathBuf::from(path)),
        _ => app_data.map(|directory| directory.join("Ricochet")),
    }
}

/// The personal directory for this process, read from `RICOCHET_DATA` and the app data folder.
pub fn default_personal_directory() -> Option<PathBuf> {
    let override_path = std::env::var("RICOCHET_DATA").ok();
    resolve_personal_directory(override_path.as_deref(), app_data_directory().as_deref())
}

/// The per-user app data folder, as .NET's `SpecialFolder.ApplicationData` gives it on each system.
pub(crate) fn app_data_directory() -> Option<PathBuf> {
    if cfg!(windows) {
        return std::env::var_os("APPDATA").filter(|value| !value.is_empty()).map(PathBuf::from);
    }

    let home = std::env::var_os("HOME").filter(|value| !value.is_empty()).map(PathBuf::from);
    if cfg!(target_os = "macos") {
        return home.map(|home| home.join("Library").join("Application Support"));
    }

    match std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|path| path.is_absolute()) {
        Some(config) => Some(config),
        None => home.map(|home| home.join(".config")),
    }
}

/// Saves the active Environment in `state.json`. Only that key changes: every other key already in the file is kept,
/// including ones this port does not model yet, so the C# app's settings survive. A missing file starts as an empty
/// object. A file that is not a JSON object is an error, so it is not written over.
pub fn save_active_environment(personal_directory: &Path, environment_id: Option<&str>) -> std::io::Result<()> {
    update_state(personal_directory, |object| {
        object.insert(
            "activeEnvironmentId".to_string(),
            environment_id.map_or(serde_json::Value::Null, |id| serde_json::Value::String(id.to_string())),
        );
    })
}

/// Saves the personal settings (ADR 0007) in `state.json`, as `SaveSettings` does in the C# store. Only the
/// `settings` key changes, and any setting this port does not model yet is kept, so the C# app's survive.
pub fn save_settings(personal_directory: &Path, settings: &PersonalSettings) -> std::io::Result<()> {
    let value = serde_json::to_value(settings).map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    update_state(personal_directory, |object| {
        let mut merged = match object.remove("settings") {
            Some(serde_json::Value::Object(existing)) => existing,
            _ => serde_json::Map::new(),
        };
        if let serde_json::Value::Object(fields) = value {
            merged.extend(fields);
        }
        object.insert("settings".to_string(), serde_json::Value::Object(merged));
    })
}

/// Reads `state.json` as an object, lets `change` edit it, and writes it back through a temporary file. A missing
/// file starts as an empty object. A file that is not a JSON object is an error, so it is not written over.
fn update_state(
    personal_directory: &Path,
    change: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>),
) -> std::io::Result<()> {
    let path = personal_directory.join(STATE_FILE_NAME);
    let mut object = match fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(serde_json::Value::Object(map)) => map,
            Ok(serde_json::Value::Null) => serde_json::Map::new(),
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "state.json is not a settings object, so it was left as it is",
                ));
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::Map::new(),
        Err(error) => return Err(error),
    };
    change(&mut object);
    let text = serde_json::to_string_pretty(&serde_json::Value::Object(object))
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    fs::create_dir_all(personal_directory)?;
    let temp = personal_directory.join(format!("{STATE_FILE_NAME}.tmp"));
    fs::write(&temp, text)?;
    fs::rename(&temp, &path)
}
