//! Importing into the open Workspace, kept out of the window so tests reach it. Ports `ImportFilesAsync`,
//! `ApplyImport`, `ImportDumpAsync` and `ApplySpecAsync` from the C# `MainViewModel` (ADR 0002, 0014, 0033, 0038):
//!
//! - Several exports can be imported at once. Each file is imported on its own, so one bad file does not stop the
//!   others, and the report lists what happened to each.
//! - A Postman data dump (a .zip) is listed first, and only the exports the person keeps ticked are imported.
//! - An OpenAPI description, fetched from a URL or read from a file, is previewed before it is applied.
//! - Imports never overwrite: a Collection or Environment whose name is taken gets a numbered name, and globals
//!   with the same name are replaced and listed.
//!
//! Nothing here writes a file. The imported items are marked unsaved, and the next save writes them.

use std::collections::HashSet;
use std::path::Path;

use ricochet_core::models::Workspace;
use ricochet_core::openapi::{self, OpenApiResult};
use ricochet_core::postman::{self, DumpEntry, ImportResult};
use ricochet_core::{insomnia, models::Collection};

/// What applying an import changed: the report text, and the ids of the items added or replaced, to mark unsaved.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Applied {
    pub report: String,
    pub changed_ids: Vec<String>,
    /// True when global Variables were added or replaced, which have no item id of their own.
    pub globals_changed: bool,
}

/// True for a Postman data dump, which opens a choice instead of importing at once.
pub fn is_dump(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
}

/// Adds one Postman export to the Workspace. Mirrors `ApplyImport`.
pub fn apply_postman(workspace: &mut Workspace, result: &ImportResult) -> Applied {
    let mut notes = Vec::new();
    let mut changed_ids = Vec::new();

    if let Some(collection) = &result.collection {
        let unique = postman::unique_name(&collection.name, workspace.collections.iter().map(|c| c.name.as_str()));
        if unique != collection.name {
            notes.push(format!("A collection with this name already exists, so it was added as \"{unique}\"."));
        }
        changed_ids.push(collection.id.clone());
        workspace.collections.push(Collection { name: unique, ..collection.clone() });
    }

    if let Some(environment) = &result.environment {
        let unique = postman::unique_name(&environment.name, workspace.environments.iter().map(|e| e.name.as_str()));
        if unique != environment.name {
            notes.push(format!("An environment with this name already exists, so it was added as \"{unique}\"."));
        }
        changed_ids.push(environment.id.clone());
        let mut environment = environment.clone();
        environment.name = unique;
        workspace.environments.push(environment);
        notes.push("Select it from the environment menu to use it.".to_string());
    }

    if let Some(globals) = &result.globals {
        let (merged, overwritten) = postman::merge_globals(&workspace.globals, globals);
        workspace.globals = merged;
        if !overwritten.is_empty() {
            notes.push(format!("Replaced existing values of: {}", overwritten.join(", ")));
        }
    }

    let text = result.to_report_text().trim_end().to_string();
    let report = if notes.is_empty() { text } else { format!("{text}\n\n{}", notes.join("\n")) };
    Applied { report, changed_ids, globals_changed: result.globals.is_some() }
}

/// Adds an Insomnia export to the Workspace, with the same rule for names as a Postman import.
pub fn apply_insomnia(workspace: &mut Workspace, result: &insomnia::InsomniaImportResult) -> Applied {
    let mut notes = Vec::new();
    let mut changed_ids = Vec::new();
    for imported in &result.workspaces {
        let collection = &imported.collection;
        let unique = postman::unique_name(&collection.name, workspace.collections.iter().map(|c| c.name.as_str()));
        if unique != collection.name {
            notes.push(format!("A collection with this name already exists, so it was added as \"{unique}\"."));
        }
        changed_ids.push(collection.id.clone());
        workspace.collections.push(Collection { name: unique, ..collection.clone() });

        for environment in &imported.environments {
            let unique = postman::unique_name(&environment.name, workspace.environments.iter().map(|e| e.name.as_str()));
            if unique != environment.name {
                notes.push(format!("An environment with this name already exists, so it was added as \"{unique}\"."));
            }
            changed_ids.push(environment.id.clone());
            let mut environment = environment.clone();
            environment.name = unique;
            workspace.environments.push(environment);
        }
    }

    let text = result.to_report_text().trim_end().to_string();
    let report = if notes.is_empty() { text } else { format!("{text}\n\n{}", notes.join("\n")) };
    Applied { report, changed_ids, globals_changed: false }
}

/// Imports one export file's text: Postman first, then Insomnia. The report starts with the file name, and says
/// "Not imported." with the reason when neither importer can read it.
pub fn import_text(workspace: &mut Workspace, text: &str, file_name: &str) -> Applied {
    let applied = match postman::import(text, file_name) {
        Ok(result) => Ok(apply_postman(workspace, &result)),
        Err(postman_error) => match insomnia::import(text, file_name) {
            Ok(result) => Ok(apply_insomnia(workspace, &result)),
            Err(_) => Err(format!("It is not a Postman or Insomnia export. {postman_error}")),
        },
    };
    match applied {
        Ok(applied) => Applied { report: format!("{file_name}\n{}", applied.report), ..applied },
        Err(error) => Applied { report: format!("{file_name}\nNot imported. {error}"), ..Applied::default() },
    }
}

/// Imports only the dump entries the person kept ticked. Mirrors `ImportDumpAsync`.
pub fn apply_dump(workspace: &mut Workspace, dump_name: &str, listing: &[DumpEntry], ticked: &HashSet<String>) -> Applied {
    let results = postman::selected(listing, ticked);
    if results.is_empty() {
        return Applied { report: format!("{dump_name}\nNothing was selected, so nothing was imported."), ..Applied::default() };
    }
    let mut changed_ids = Vec::new();
    let mut reports = Vec::new();
    let mut globals_changed = false;
    for result in results {
        let applied = apply_postman(workspace, result);
        changed_ids.extend(applied.changed_ids);
        globals_changed |= applied.globals_changed;
        reports.push(applied.report);
    }
    Applied { report: format!("{dump_name}\n\n{}", reports.join("\n\n")), changed_ids, globals_changed }
}

/// Builds the preview of an OpenAPI description. An earlier import from the same address is updated rather than
/// added again. Mirrors `FetchSpecAsync` after the fetch.
///
/// # Errors
/// The message to show when the text is not an OpenAPI 3 description in JSON.
pub fn preview_spec(workspace: &Workspace, text: &str, address: &str) -> Result<OpenApiResult, String> {
    let existing = workspace.collections.iter().find(|c| c.spec_url.as_deref() == Some(address)).cloned();
    openapi::build(text, address, existing).map_err(|error| error.message().to_string())
}

/// Applies a previewed description: a new Collection is added under a free name, and an update replaces the
/// Collection that came from the same address. Mirrors `ApplySpecAsync`.
pub fn apply_spec(workspace: &mut Workspace, result: OpenApiResult) -> Applied {
    let address = result.collection.spec_url.clone().unwrap_or_default();
    let report = format!("From {address}\n\n{}", result.summary.to_report_text().trim_end());
    let id = result.collection.id.clone();
    if result.summary.is_new {
        let unique = postman::unique_name(&result.collection.name, workspace.collections.iter().map(|c| c.name.as_str()));
        workspace.collections.push(Collection { name: unique, ..result.collection });
    } else if let Some(old) = workspace.collections.iter_mut().find(|c| c.spec_url.as_deref() == Some(address.as_str())) {
        *old = result.collection;
    }
    Applied { report, changed_ids: vec![id], globals_changed: false }
}
