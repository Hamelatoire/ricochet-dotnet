//! The command palette's entries and how they are filtered (ADR 0091, the C# MainViewModel.Palette). Plain functions on
//! the Core model, so tests reach them without a window. The window runs the chosen action.

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteEntry {
    pub label: String,
    pub action: PaletteAction,
}

/// Every entry the palette offers for this Workspace, in a fixed order: commands, Environments, then Requests.
pub fn entries(workspace: &Workspace) -> Vec<PaletteEntry> {
    let mut all = vec![
        PaletteEntry { label: "Send request".to_string(), action: PaletteAction::Send },
        PaletteEntry { label: "Save".to_string(), action: PaletteAction::Save },
        PaletteEntry { label: "Close all tabs".to_string(), action: PaletteAction::CloseAllTabs },
        PaletteEntry { label: "Close other tabs".to_string(), action: PaletteAction::CloseOtherTabs },
        PaletteEntry { label: "Run collection".to_string(), action: PaletteAction::RunCollection },
        PaletteEntry { label: "New request".to_string(), action: PaletteAction::NewRequest },
        PaletteEntry { label: "Use no environment".to_string(), action: PaletteAction::UseEnvironment(None) },
    ];
    for environment in &workspace.environments {
        all.push(PaletteEntry {
            label: format!("Use environment: {}", environment.name.trim()),
            action: PaletteAction::UseEnvironment(Some(environment.id.clone())),
        });
    }
    for row in build_tree(workspace, false).into_iter().filter(|row| row.kind == RowKind::Request) {
        all.push(PaletteEntry { label: format!("Open: {}", row.label), action: PaletteAction::OpenRequest(row.id) });
    }
    all
}

/// The entries whose label contains every word of the query, ignoring case. An empty query keeps every entry.
pub fn filter(entries: &[PaletteEntry], query: &str) -> Vec<PaletteEntry> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    entries
        .iter()
        .filter(|entry| {
            let label = entry.label.to_lowercase();
            words.iter().all(|word| label.contains(word))
        })
        .cloned()
        .collect()
}
