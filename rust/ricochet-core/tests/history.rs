//! Tests for History: the file, its cap, and hiding literal credentials.

use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, FixedOffset};
use pretty_assertions::assert_eq;
use ricochet_core::history::{self, HIDDEN_VALUE};
use ricochet_core::models::{ApiRequest, HistoryEntry, KeyValue, MAX_HISTORY};

fn entry(name: &str) -> HistoryEntry {
    let at: DateTime<FixedOffset> = DateTime::parse_from_rfc3339("2026-11-10T12:00:00+00:00").unwrap();
    HistoryEntry {
        request: ApiRequest { name: name.into(), url: "https://a.test".into(), ..Default::default() },
        status_code: Some(200),
        elapsed_ms: 12,
        at,
        collection_id: None,
        certificates_unverified: None,
    }
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("ricochet-history-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn recording_puts_the_newest_first_and_keeps_at_most_the_cap() {
    let mut entries = Vec::new();
    for i in 0..MAX_HISTORY + 3 {
        history::record(&mut entries, entry(&format!("r{i}")));
    }
    assert_eq!(entries.len(), MAX_HISTORY);
    assert_eq!(entries[0].request.name, format!("r{}", MAX_HISTORY + 2));
    assert_eq!(entries.last().unwrap().request.name, "r3");
}

#[test]
fn a_missing_file_is_an_empty_history_and_saved_entries_read_back() {
    let dir = temp_dir("round");
    assert!(history::load(&dir).unwrap().is_empty());
    let entries = vec![entry("first"), entry("second")];
    history::save(&dir, &entries).unwrap();
    assert_eq!(history::load(&dir).unwrap(), entries);
    assert!(!dir.join("history.json.tmp").exists());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_damaged_file_is_an_error_so_it_is_not_written_over() {
    let dir = temp_dir("damaged");
    std::fs::write(dir.join("history.json"), "not json").unwrap();
    assert!(history::load(&dir).is_err());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_literal_credential_is_hidden_and_a_placeholder_is_kept() {
    let request = ApiRequest {
        headers: vec![
            KeyValue::new("Authorization", "Bearer abc123"),
            KeyValue::new("X-Api-Key", "{{apiKey}}"),
            KeyValue::new("Accept", "application/json"),
        ],
        ..Default::default()
    };
    let shown = history::redacted(&request);
    assert_eq!(shown.headers[0].value, HIDDEN_VALUE);
    assert_eq!(shown.headers[1].value, "{{apiKey}}");
    assert_eq!(shown.headers[2].value, "application/json");
    assert_eq!(request.headers[0].value, "Bearer abc123", "the original is not changed");
}
