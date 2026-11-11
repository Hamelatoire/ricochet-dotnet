//! The mock server page's logic (ADR 0023, ADR 0027, ADR 0091): turns the saved response examples of a Workspace,
//! or a loaded OpenAPI description, into routes, and keeps the log of answered requests. Plain functions, so tests
//! reach them without a window.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use ricochet_core::mock_server::{MockExample, MockLogEntry, MockRouteTable, routes_from_examples, routes_from_openapi};
use ricochet_core::models::{CollectionItem, Workspace};

/// Every saved example of every Request, in tree order, as something a mock can answer with.
pub fn examples_of(workspace: &Workspace) -> Vec<MockExample> {
    let mut found = Vec::new();
    for collection in &workspace.collections {
        collect(&collection.items, &mut found);
    }
    found
}

fn collect(items: &[CollectionItem], found: &mut Vec<MockExample>) {
    for item in items {
        if let Some(request) = &item.request {
            for example in request.examples.iter().flatten() {
                found.push(MockExample {
                    method: request.method.clone(),
                    url: request.url.clone(),
                    status_code: example.status_code.clamp(100, 599) as u16,
                    media_type: example.media_type.clone(),
                    body: example.body.clone(),
                    headers: example.headers.clone(),
                });
            }
        }
        collect(&item.children, found);
    }
}

/// The route table for the Workspace's examples.
pub fn table_of(workspace: &Workspace) -> MockRouteTable {
    MockRouteTable::new(routes_from_examples(&examples_of(workspace)))
}

/// An OpenAPI description loaded for the mock server. While one is loaded, it answers instead of the saved examples,
/// as in the C# app, where the description is the only source.
#[derive(Debug, Clone, PartialEq)]
pub struct MockDescription {
    pub file_name: String,
    pub table: MockRouteTable,
}

/// Builds routes from the text of an OpenAPI description. Mirrors `MockServerViewModel.LoadDescription`: the error is
/// Core's message, such as "This is not an OpenAPI 3 description."
pub fn load_description(json: &str, file_name: &str) -> Result<MockDescription, String> {
    let routes = routes_from_openapi(json.trim_start_matches('\u{feff}')).map_err(|error| error.message().to_string())?;
    Ok(MockDescription { file_name: file_name.to_string(), table: MockRouteTable::new(routes) })
}

/// Reads the file the user picked. The file is only read, and nothing is sent anywhere. Mirrors
/// `LoadMockDescriptionFromFileAsync`.
pub fn read_description(path: &Path) -> Result<MockDescription, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let bytes = std::fs::read(path).map_err(|error| format!("The file could not be read: {error}"))?;
    load_description(&String::from_utf8_lossy(&bytes), &name)
}

/// The routes the server answers with: the loaded description's, else the saved examples'.
pub fn active_table(workspace: &Workspace, description: Option<&MockDescription>) -> MockRouteTable {
    match description {
        Some(description) => description.table.clone(),
        None => table_of(workspace),
    }
}

/// The routes to start the server with, or why it cannot start: nothing would answer.
pub fn start_table(workspace: &Workspace, description: Option<&MockDescription>) -> Result<MockRouteTable, String> {
    let table = active_table(workspace, description);
    if !table.is_empty() {
        return Ok(table);
    }
    Err(match description {
        Some(description) => format!("{} has no operations to answer with. Load another description.", description.file_name),
        None => "There are no saved examples to answer with. Use Save example on a response first, or load an OpenAPI file.".to_string(),
    })
}

/// What the server answers from, such as "petstore.json: 3 routes". Mirrors `MockServerViewModel.SourceText`.
pub fn source_text(workspace: &Workspace, description: Option<&MockDescription>) -> String {
    let (source, count) = match description {
        Some(description) => (description.file_name.clone(), description.table.len()),
        None => ("Saved examples".to_string(), table_of(workspace).len()),
    };
    format!("{source}: {count} {}", if count == 1 { "route" } else { "routes" })
}

/// The answered requests, newest last, shared between the server thread and the window.
#[derive(Clone, Default)]
pub struct MockLog {
    entries: Arc<Mutex<Vec<MockLogEntry>>>,
}

/// How many answered requests the log keeps.
pub const LOG_LIMIT: usize = 200;

impl MockLog {
    pub fn push(&self, entry: MockLogEntry) {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries.push(entry);
        let extra = entries.len().saturating_sub(LOG_LIMIT);
        entries.drain(..extra);
    }

    pub fn entries(&self) -> Vec<MockLogEntry> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn clear(&self) {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner).clear();
    }
}

/// Reads a port typed in the window. Spaces around it are ignored.
pub fn parse_port(text: &str) -> Result<u16, String> {
    text.trim().parse::<u16>().map_err(|_| "Type a port number, such as 5050.".to_string())
}
