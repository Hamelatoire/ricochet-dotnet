//! The mock server page's logic (ADR 0023, ADR 0091): turns the saved response examples of a Workspace into routes,
//! and keeps the log of answered requests. Plain functions, so tests reach them without a window.

use std::sync::{Arc, Mutex, PoisonError};

use ricochet_core::mock_server::{MockExample, MockLogEntry, MockRouteTable, routes_from_examples};
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
