//! Port of `EnvironmentTransfer.cs` (ADR 0036). Moves one Environment in and out of a file.
//! Secret values are left out on export and never written. The Postman path is not ported yet, so
//! Postman environment files are refused with a message until the Postman importer lands.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::{ApiEnvironment, KeyValue, new_id};

pub const FORMAT_NAME: &str = "ricochet-environment";
pub const FORMAT_VERSION: u32 = 1;

/// Issue category for names changed to fit the variable name rule.
pub const RENAMED_VARIABLES: &str = "Variable names changed to fit";
/// Issue category for Secrets that the file leaves without a value.
pub const SECRETS: &str = "Variables marked as Secret, so kept out of your files";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentImportError {
    pub message: String,
}

impl EnvironmentImportError {
    fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

impl std::fmt::Display for EnvironmentImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for EnvironmentImportError {}

/// What to do when an imported Environment has the same name as one already in the Workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvironmentClash {
    /// Overwrite the existing Environment, keeping its id so local Secret values still apply.
    Replace,
    /// Add the Environment under a numbered name, such as "Staging (2)".
    Copy,
    /// Leave the existing Environment as it is.
    Skip,
}

/// A note about something the import changed or could not carry over.
/// Mirrors `ImportIssue` in C#; it can be replaced by the Postman importer's type when that lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportIssue {
    pub category: String,
    pub location: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnvironmentImportResult {
    pub environment: ApiEnvironment,
    pub summary: String,
    pub issues: Vec<ImportIssue>,
}

/// The result of placing an imported Environment. `placed` is None when skipped.
#[derive(Debug, Clone, PartialEq)]
pub struct EnvironmentPlacement {
    pub environments: Vec<ApiEnvironment>,
    pub placed: Option<ApiEnvironment>,
}

/// An Environment as written by [`export`]. Secret values are never written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EnvironmentFile {
    pub format: String,
    pub version: u32,
    pub name: String,
    pub variables: Vec<EnvironmentFileVariable>,
}

impl Default for EnvironmentFile {
    fn default() -> Self {
        Self {
            format: FORMAT_NAME.to_string(),
            version: FORMAT_VERSION,
            name: String::new(),
            variables: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EnvironmentFileVariable {
    pub key: String,
    /// None for a Secret, so the file holds the name but no value. Omitted from the file when None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub is_secret: bool,
}

impl Default for EnvironmentFileVariable {
    fn default() -> Self {
        Self { key: String::new(), value: None, enabled: true, is_secret: false }
    }
}

fn yes() -> bool {
    true
}

/// Writes one Environment as a `ricochet-environment` version 1 file, indented like the C# app.
/// Secret variables keep their name, enabled flag and `is_secret`, but have no value.
pub fn export(environment: &ApiEnvironment) -> String {
    let file = EnvironmentFile {
        name: environment.name.clone(),
        variables: environment
            .variables
            .iter()
            .map(|v| EnvironmentFileVariable {
                key: v.key.clone(),
                value: if v.is_secret { None } else { Some(v.value.clone()) },
                enabled: v.enabled,
                is_secret: v.is_secret,
            })
            .collect(),
        ..EnvironmentFile::default()
    };

    serde_json::to_string_pretty(&file).expect("an environment file always serializes")
}

/// How many Secret variables an Environment has. Their values are never exported or imported.
pub fn secret_count(environment: &ApiEnvironment) -> usize {
    environment.variables.iter().filter(|v| v.is_secret).count()
}

/// Finds the Environment with this name, ignoring case. None when there is none, or more than one.
pub fn find_environment<'a>(environments: &'a [ApiEnvironment], name: &str) -> Option<&'a ApiEnvironment> {
    let mut matches = environments.iter().filter(|e| same_name(&e.name, name));
    let first = matches.next()?;
    if matches.next().is_some() { None } else { Some(first) }
}

/// Reads a Ricochet environment file. Postman files are refused until the Postman importer is ported.
pub fn import(json: &str, file_name: &str) -> Result<EnvironmentImportResult, EnvironmentImportError> {
    let value: Value = serde_json::from_str(json)
        .map_err(|e| EnvironmentImportError::new(format!("Not valid JSON: {e}")))?;

    let is_ours = value
        .as_object()
        .and_then(|object| object.get("format"))
        .and_then(Value::as_str)
        == Some(FORMAT_NAME);

    if is_ours {
        read_own_file(value, file_name)
    } else {
        Err(EnvironmentImportError::new(
            "Postman environment files cannot be imported yet. The Postman importer has not been ported to Rust.",
        ))
    }
}

/// Adds an imported Environment to the existing ones. The existing list is not modified.
/// A name clash is resolved with `clash`; without a clash the Environment is simply added.
pub fn place(existing: &[ApiEnvironment], incoming: ApiEnvironment, clash: EnvironmentClash) -> EnvironmentPlacement {
    let Some(index) = existing.iter().position(|e| same_name(&e.name, &incoming.name)) else {
        let mut environments = existing.to_vec();
        environments.push(incoming.clone());
        return EnvironmentPlacement { environments, placed: Some(incoming) };
    };

    match clash {
        EnvironmentClash::Skip => EnvironmentPlacement { environments: existing.to_vec(), placed: None },
        EnvironmentClash::Copy => {
            let copy = ApiEnvironment {
                id: new_id(),
                name: unique_name(&incoming.name, existing.iter().map(|e| e.name.as_str())),
                ..incoming
            };
            let mut environments = existing.to_vec();
            environments.push(copy.clone());
            EnvironmentPlacement { environments, placed: Some(copy) }
        }
        EnvironmentClash::Replace => {
            let previous = &existing[index];
            let replacement = ApiEnvironment {
                id: previous.id.clone(),
                variables: keep_local_secrets(&incoming.variables, &previous.variables),
                ..incoming
            };
            let mut environments = existing.to_vec();
            environments[index] = replacement.clone();
            EnvironmentPlacement { environments, placed: Some(replacement) }
        }
    }
}

fn read_own_file(value: Value, file_name: &str) -> Result<EnvironmentImportResult, EnvironmentImportError> {
    let file: EnvironmentFile = serde_json::from_value(value)
        .map_err(|e| EnvironmentImportError::new(format!("Not a readable environment file: {e}")))?;

    if file.version > FORMAT_VERSION {
        return Err(EnvironmentImportError::new(format!(
            "This environment file uses format version {}, which is newer than this app reads ({FORMAT_VERSION}).",
            file.version
        )));
    }

    let name = if file.name.is_empty() {
        Path::new(file_name).file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string()
    } else {
        file.name
    };

    let mut issues = Vec::new();
    let mut variables = Vec::new();
    for variable in file.variables {
        if variable.key.is_empty() {
            continue;
        }

        let key = fit_name(&variable.key, &name, &mut issues);
        let value = variable.value.unwrap_or_default();
        if variable.is_secret && value.is_empty() {
            issues.push(ImportIssue {
                category: SECRETS.to_string(),
                location: name.clone(),
                detail: format!("{key} (the export has no value, so set it again)"),
            });
        }

        variables.push(KeyValue {
            key,
            value,
            enabled: variable.enabled,
            is_secret: variable.is_secret,
            ..KeyValue::default()
        });
    }

    Ok(build(name, variables, issues))
}

fn build(name: String, variables: Vec<KeyValue>, issues: Vec<ImportIssue>) -> EnvironmentImportResult {
    let noun = if variables.len() == 1 { "variable" } else { "variables" };
    let summary = format!("Imported environment \"{name}\" with {} {noun}.", variables.len());
    let environment = ApiEnvironment { name, variables, ..ApiEnvironment::default() };
    EnvironmentImportResult { environment, summary, issues }
}

/// Variable names may only use letters, digits, dash, underscore and dot. Other characters become
/// underscores, and the change is reported.
fn fit_name(key: &str, environment_name: &str, issues: &mut Vec<ImportIssue>) -> String {
    if is_valid_name(key) {
        return key.to_string();
    }

    let fitted: String = key.chars().map(|c| if is_name_char(c) { c } else { '_' }).collect();
    issues.push(ImportIssue {
        category: RENAMED_VARIABLES.to_string(),
        location: environment_name.to_string(),
        detail: format!("\"{key}\" is now \"{fitted}\""),
    });
    fitted
}

fn is_valid_name(key: &str) -> bool {
    !key.is_empty() && key.chars().all(is_name_char)
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')
}

/// A Secret the file leaves without a value keeps the value this machine already has for it.
fn keep_local_secrets(incoming: &[KeyValue], local: &[KeyValue]) -> Vec<KeyValue> {
    let known: HashMap<&str, &str> = local
        .iter()
        .filter(|v| v.is_secret && !v.value.is_empty())
        .map(|v| (v.key.as_str(), v.value.as_str()))
        .collect();

    incoming
        .iter()
        .map(|v| {
            match known.get(v.key.as_str()) {
                Some(value) if v.is_secret && v.value.is_empty() => KeyValue { value: value.to_string(), ..v.clone() },
                _ => v.clone(),
            }
        })
        .collect()
}

/// `name`, or `name (2)`, `name (3)` and so on, whichever is not taken, ignoring case.
fn unique_name<'a>(name: &str, existing: impl Iterator<Item = &'a str>) -> String {
    let taken: HashSet<String> = existing.map(|n| n.to_lowercase()).collect();
    if !taken.contains(&name.to_lowercase()) {
        return name.to_string();
    }

    (2..)
        .map(|n| format!("{name} ({n})"))
        .find(|candidate| !taken.contains(&candidate.to_lowercase()))
        .expect("the iterator is unbounded")
}

fn same_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}
