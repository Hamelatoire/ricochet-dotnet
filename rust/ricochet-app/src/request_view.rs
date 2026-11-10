//! What the request pane draws from a Request: its URL split around `{{placeholders}}`, its query parameters, and the
//! list of sends made in this window. Plain data, so tests reach it without a window.

use ricochet_core::models::KeyValue;
use ricochet_core::placeholder_chips;

/// One piece of a URL: plain text, or a placeholder drawn as a chip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlSegment {
    Text(String),
    /// `resolves` is false when nothing in scope gives the name a value.
    Chip { name: String, resolves: bool },
}

/// Splits a URL around its placeholders, in order. `variables` is every scope layered lowest first.
pub fn url_segments(url: &str, variables: &[KeyValue]) -> Vec<UrlSegment> {
    let mut segments = Vec::new();
    let mut at = 0;
    for chip in placeholder_chips::find(url, variables) {
        if chip.start > at {
            segments.push(UrlSegment::Text(url[at..chip.start].to_string()));
        }
        at = chip.end();
        segments.push(UrlSegment::Chip { name: chip.name, resolves: chip.resolves });
    }
    if at < url.len() {
        segments.push(UrlSegment::Text(url[at..].to_string()));
    }
    segments
}

/// The query parameters written in a URL, as typed: nothing is decoded, and a fragment is left out.
pub fn query_params(url: &str) -> Vec<(String, String)> {
    let without_fragment = url.split('#').next().unwrap_or("");
    let Some((_, query)) = without_fragment.split_once('?') else {
        return Vec::new();
    };
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((key, value)) => (key.to_string(), value.to_string()),
            None => (pair.to_string(), String::new()),
        })
        .collect()
}

/// Most sends kept in the window's list of recent sends.
pub const MAX_RECENT_SENDS: usize = 30;

/// One send made in this window. It is not the Workspace's History: it is never saved and holds no Request copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentSend {
    pub request_id: String,
    pub name: String,
    pub method: String,
    /// None when the send failed before any response.
    pub status_code: Option<u16>,
    pub elapsed_ms: u128,
}

/// The sends made in this window, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecentSends {
    entries: Vec<RecentSend>,
}

impl RecentSends {
    pub fn entries(&self) -> &[RecentSend] {
        &self.entries
    }

    /// Adds a send at the top and drops the oldest past [`MAX_RECENT_SENDS`].
    pub fn record(&mut self, send: RecentSend) {
        self.entries.insert(0, send);
        self.entries.truncate(MAX_RECENT_SENDS);
    }
}
