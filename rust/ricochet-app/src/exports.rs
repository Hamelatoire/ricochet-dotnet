//! Exports a Collection as Postman or OpenAPI JSON (ADR 0091). Plain functions over the core exporters, so the text
//! can be tested without a window. Choosing a file and writing it is the window's job.

use ricochet_core::models::Collection;
use ricochet_core::{openapi_export, postman_export};

/// Which format a Collection is exported to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Postman,
    OpenApi,
}

impl ExportFormat {
    /// The file name suggested for an export of this format.
    pub fn suggested_file_name(self, collection: &Collection) -> String {
        let base = postman_export_name(collection);
        match self {
            ExportFormat::Postman => format!("{base}.postman_collection.json"),
            ExportFormat::OpenApi => format!("{base}.openapi.json"),
        }
    }
}

/// The export of a Collection as JSON text, with the notes the exporter gives about what could not be carried over.
pub struct Exported {
    pub json: String,
    /// The exporter's notes, grouped by category, ready to show. Empty when everything carried over.
    pub notes: String,
}

/// Exports the Collection in the chosen format.
pub fn export(collection: &Collection, format: ExportFormat) -> Exported {
    match format {
        ExportFormat::Postman => {
            let result = postman_export::export(collection);
            Exported { json: result.json.clone(), notes: notes_text(result.notes.is_empty(), &result.to_report_text()) }
        }
        ExportFormat::OpenApi => {
            let result = openapi_export::export(collection);
            Exported { json: result.json.clone(), notes: notes_text(result.notes.is_empty(), &result.report_text()) }
        }
    }
}

fn notes_text(none: bool, text: &str) -> String {
    if none { String::new() } else { text.to_string() }
}

/// A file-name stem from the Collection's name, falling back to a plain word when the name has no usable characters.
fn postman_export_name(collection: &Collection) -> String {
    let cleaned: String = collection
        .name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let mut collapsed = String::new();
    for c in cleaned.chars() {
        if c == '-' && collapsed.ends_with('-') {
            continue;
        }
        collapsed.push(c);
    }
    let trimmed = collapsed.trim_matches('-');
    if trimmed.is_empty() { "collection".to_string() } else { trimmed.to_string() }
}
