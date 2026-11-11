//! The command palette's entries and how they are filtered (ADR 0091, the C# MainViewModel.Palette). Plain functions on
//! the Core model, so tests reach them without a window. The window runs the chosen action.

use ricochet_core::command_palette;
use ricochet_core::models::Workspace;

use crate::tree::{RowKind, build_tree};

/// What choosing a palette entry does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteAction {
    Send,
    Save,
    NewRequest,
    /// Opens a Request in a tab, by its item id.
    OpenRequest(String),
    /// Uses this Environment, or none.
    UseEnvironment(Option<String>),
    CloseAllTabs,
    CloseOtherTabs,
    /// Runs the Collection of the sidebar's selection, with the collection runner.
    RunCollection,
    /// Opens the bulk run panel for the selected Request.
    BulkRun,
    /// Exports the Collection of the sidebar's selection as Postman JSON, or as OpenAPI JSON.
    ExportPostman,
    ExportOpenApi,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteEntry {
    pub label: String,
    pub action: PaletteAction,
}

/// Every entry the palette offers for this Workspace, in a fixed order: commands, Environments, then Requests.
pub fn entries(workspace: &Workspace) -> Vec<PaletteEntry> {
    entries_for(workspace, false)
}

/// The entries for this Workspace. With `requests_only` set (Ctrl+P, the C# Find a request), only the Requests are
/// offered, as the C# palette does when it is opened that way.
pub fn entries_for(workspace: &Workspace, requests_only: bool) -> Vec<PaletteEntry> {
    let mut all = Vec::new();
    if !requests_only {
        all.extend(action_entries(workspace));
    }
    all.extend(request_entries(workspace));
    all
}

fn action_entries(workspace: &Workspace) -> Vec<PaletteEntry> {
    let mut all = vec![
        PaletteEntry { label: "Send request".to_string(), action: PaletteAction::Send },
        PaletteEntry { label: "Save".to_string(), action: PaletteAction::Save },
        PaletteEntry { label: "Close all tabs".to_string(), action: PaletteAction::CloseAllTabs },
        PaletteEntry { label: "Close other tabs".to_string(), action: PaletteAction::CloseOtherTabs },
        PaletteEntry { label: "Run collection".to_string(), action: PaletteAction::RunCollection },
        PaletteEntry { label: "Bulk run: send the request once for each id".to_string(), action: PaletteAction::BulkRun },
        PaletteEntry { label: "Export collection as Postman".to_string(), action: PaletteAction::ExportPostman },
        PaletteEntry { label: "Export collection as OpenAPI".to_string(), action: PaletteAction::ExportOpenApi },
        PaletteEntry { label: "New request".to_string(), action: PaletteAction::NewRequest },
        PaletteEntry { label: "Use no environment".to_string(), action: PaletteAction::UseEnvironment(None) },
    ];
    for environment in &workspace.environments {
        all.push(PaletteEntry {
            label: format!("Use environment: {}", environment.name.trim()),
            action: PaletteAction::UseEnvironment(Some(environment.id.clone())),
        });
    }
    all
}

fn request_entries(workspace: &Workspace) -> Vec<PaletteEntry> {
    build_tree(workspace, false)
        .into_iter()
        .filter(|row| row.kind == RowKind::Request)
        .map(|row| PaletteEntry { label: format!("Open: {}", row.label), action: PaletteAction::OpenRequest(row.id) })
        .collect()
}

/// The entries whose label contains every word of the query, ignoring case. An empty query keeps every entry.
/// Labels that start with the first word come first, as the C# `CommandPalette.Filter` orders them (ADR 0040).
pub fn filter(entries: &[PaletteEntry], query: &str) -> Vec<PaletteEntry> {
    command_palette::filter(entries.iter(), query, |entry: &&PaletteEntry| entry.label.as_str())
        .into_iter()
        .cloned()
        .collect()
}
