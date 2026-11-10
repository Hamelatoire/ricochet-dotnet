//! The History of sends, kept in the personal folder as `history.json` and never shared (ADR 0079 and the store's
//! personal files). Newest first, at most [`MAX_HISTORY`] entries. A credential typed into a header is hidden before it
//! is recorded, as the C# app does, so History never holds a literal credential.

use std::fs;
use std::io;
use std::path::Path;

use crate::collection_export::{is_literal, is_sensitive_header};
use crate::models::{ApiRequest, HistoryEntry, MAX_HISTORY};

/// The file name in the personal folder. The same name the C# app uses.
pub const HISTORY_FILE_NAME: &str = "history.json";
/// What a hidden header value is written as in History.
pub const HIDDEN_VALUE: &str = "[hidden]";

/// Reads History. A missing file is an empty History. A file that cannot be read is an error, so the caller does not
/// write over it.
pub fn load(personal_dir: &Path) -> io::Result<Vec<HistoryEntry>> {
    match fs::read_to_string(personal_dir.join(HISTORY_FILE_NAME)) {
        Ok(text) => serde_json::from_str(&text).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

/// Writes History, newest first, keeping at most [`MAX_HISTORY`] entries. Written to a temporary file and moved into
/// place, so a crash cannot leave half a file.
pub fn save(personal_dir: &Path, entries: &[HistoryEntry]) -> io::Result<()> {
    fs::create_dir_all(personal_dir)?;
    let kept: Vec<&HistoryEntry> = entries.iter().take(MAX_HISTORY).collect();
    let text = serde_json::to_string_pretty(&kept).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let path = personal_dir.join(HISTORY_FILE_NAME);
    let temp = personal_dir.join(format!("{HISTORY_FILE_NAME}.tmp"));
    fs::write(&temp, text)?;
    fs::rename(&temp, &path)
}

/// Adds an entry at the top, dropping the oldest past [`MAX_HISTORY`].
pub fn record(entries: &mut Vec<HistoryEntry>, entry: HistoryEntry) {
    entries.insert(0, entry);
    entries.truncate(MAX_HISTORY);
}

/// The Request as History keeps it: a header whose name is sensitive and whose value is a literal credential shows
/// [`HIDDEN_VALUE`]. Placeholders are kept, because they are safe to show.
pub fn redacted(request: &ApiRequest) -> ApiRequest {
    let mut copy = request.clone();
    for header in &mut copy.headers {
        if is_sensitive_header(&header.key) && is_literal(&header.value) {
            header.value = HIDDEN_VALUE.to_string();
        }
    }
    copy
}
