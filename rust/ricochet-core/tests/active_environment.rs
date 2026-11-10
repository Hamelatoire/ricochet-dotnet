//! Tests for saving the active Environment in state.json, keeping the other keys.

use std::time::{SystemTime, UNIX_EPOCH};

use pretty_assertions::assert_eq;
use ricochet_core::personal_state::{load, save_active_environment};

fn temp_dir(name: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("ricochet-env-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_new_file_holds_the_environment_and_loads_back() {
    let dir = temp_dir("new");
    save_active_environment(&dir, Some("staging")).unwrap();
    assert_eq!(load(&dir).state.active_environment_id.as_deref(), Some("staging"));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn other_keys_in_the_file_are_kept_and_only_the_environment_changes() {
    let dir = temp_dir("keep");
    std::fs::write(
        dir.join("state.json"),
        r#"{"theme":"dark","activeEnvironmentId":"old","someFutureSetting":{"x":1}}"#,
    )
    .unwrap();
    save_active_environment(&dir, Some("prod")).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("state.json")).unwrap()).unwrap();
    assert_eq!(saved["theme"], "dark");
    assert_eq!(saved["someFutureSetting"], serde_json::json!({"x": 1}));
    assert_eq!(saved["activeEnvironmentId"], "prod");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn no_environment_is_saved_as_null() {
    let dir = temp_dir("none");
    save_active_environment(&dir, Some("prod")).unwrap();
    save_active_environment(&dir, None).unwrap();
    assert_eq!(load(&dir).state.active_environment_id, None);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_file_that_is_not_an_object_is_not_written_over() {
    let dir = temp_dir("array");
    std::fs::write(dir.join("state.json"), "[1,2,3]").unwrap();
    assert!(save_active_environment(&dir, Some("prod")).is_err());
    assert_eq!(std::fs::read_to_string(dir.join("state.json")).unwrap(), "[1,2,3]");
    std::fs::remove_dir_all(dir).ok();
}
