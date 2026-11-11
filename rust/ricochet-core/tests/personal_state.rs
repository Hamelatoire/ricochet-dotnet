//! Tests for the personal state reader, ported from the personal `state.json` rules in `WorkspaceStore.cs`.
//! Each test works in its own temporary folder, which is removed afterwards. None of them change the environment,
//! so `RICOCHET_DATA` is passed in as a value.

use std::fs;
use std::path::{Path, PathBuf};

use pretty_assertions::assert_eq;
use ricochet_core::models::new_id;
use ricochet_core::personal_state::*;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("ricochet-personal-{tag}-{}", new_id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_state(dir: &Path, text: &str) {
    fs::write(dir.join(STATE_FILE_NAME), text).unwrap();
}

#[test]
fn a_missing_state_file_gives_the_defaults_with_no_issue() {
    let dir = TempDir::new("missing");

    let loaded = load(dir.path());

    assert_eq!(loaded.state, PersonalState::default());
    assert_eq!(loaded.issues, vec![]);
    assert!(!dir.path().join(STATE_FILE_NAME).exists());
}

#[test]
fn an_empty_object_gives_the_settings_defaults() {
    let dir = TempDir::new("empty-object");
    write_state(dir.path(), "{}");

    let loaded = load(dir.path());

    assert_eq!(loaded.issues, vec![]);
    assert_eq!(loaded.state.settings, PersonalSettings {
        request_timeout_seconds: 120,
        follow_redirects: true,
        verify_tls_certificates: true,
        max_response_megabytes: 100,
        wrap_response: true,
        proxy_url: String::new(),
        client_certificate_path: String::new(),
    });
    assert_eq!(loaded.state.theme, None);
    assert_eq!(loaded.state.bulk, vec![]);
    assert_eq!(loaded.state.window(), None);
    assert_eq!(loaded.state.open_tabs(), Vec::<String>::new());
}

#[test]
fn a_file_that_holds_only_null_gives_the_defaults() {
    let dir = TempDir::new("null");
    write_state(dir.path(), "null");

    let loaded = load(dir.path());

    assert_eq!(loaded.state, PersonalState::default());
    assert_eq!(loaded.issues, vec![]);
    assert!(dir.path().join(STATE_FILE_NAME).exists(), "a null file is not moved aside");
}

#[test]
fn reads_the_saved_fields_in_camel_case() {
    let dir = TempDir::new("fields");
    write_state(
        dir.path(),
        r#"{
            "workspaceFolder": "D:/work/api",
            "activeEnvironmentId": "env-1",
            "theme": "dark",
            "bulk": [{ "requestId": "r1", "ids": "1,2,3", "lastId": "2", "processed": 2 }],
            "settings": { "requestTimeoutSeconds": 30, "followRedirects": false, "proxyUrl": "http://proxy.example:8080" },
            "openTabs": ["r1", "r2"],
            "activeTab": "r2"
        }"#,
    );

    let state = load(dir.path()).state;

    assert_eq!(state.workspace_folder.as_deref(), Some("D:/work/api"));
    assert_eq!(state.active_environment_id.as_deref(), Some("env-1"));
    assert_eq!(state.theme.as_deref(), Some("dark"));
    assert_eq!(state.bulk, vec![BulkProgress {
        request_id: "r1".into(),
        ids: "1,2,3".into(),
        last_id: Some("2".into()),
        processed: 2,
    }]);
    assert_eq!(state.settings.request_timeout_seconds, 30);
    assert!(!state.settings.follow_redirects);
    assert!(state.settings.verify_tls_certificates, "a missing setting keeps its default");
    assert_eq!(state.settings.max_response_megabytes, 100);
    assert_eq!(state.settings.proxy_url, "http://proxy.example:8080");
    assert_eq!(state.open_tabs(), vec!["r1".to_string(), "r2".to_string()]);
    assert_eq!(state.active_tab.as_deref(), Some("r2"));
}

#[test]
fn a_state_file_from_before_settings_existed_gets_the_settings_defaults() {
    let dir = TempDir::new("theme-only");
    write_state(dir.path(), r#"{ "theme": "dark" }"#);

    let loaded = load(dir.path());

    assert_eq!(loaded.issues, vec![]);
    assert_eq!(loaded.state.theme.as_deref(), Some("dark"));
    assert_eq!(loaded.state.settings, PersonalSettings::default());
}

#[test]
fn unknown_fields_are_ignored() {
    let dir = TempDir::new("unknown");
    write_state(dir.path(), r#"{ "theme": "light", "somethingNew": { "a": 1 } }"#);

    let loaded = load(dir.path());

    assert_eq!(loaded.state.theme.as_deref(), Some("light"));
    assert_eq!(loaded.issues, vec![]);
}

#[test]
fn invalid_json_is_set_aside_and_reported_with_the_defaults() {
    let dir = TempDir::new("invalid-json");
    write_state(dir.path(), "{ \"theme\": ");

    let loaded = load(dir.path());

    assert_eq!(loaded.state, PersonalState::default());
    assert_eq!(loaded.issues.len(), 1);
    assert!(loaded.issues[0].problem.starts_with("This file could not be read, so it was set aside"));
    assert!(!dir.path().join(STATE_FILE_NAME).exists(), "the original is moved, not left in place");

    let kept = PathBuf::from(&loaded.issues[0].path);
    assert!(kept.exists(), "the set-aside copy exists at the reported path");
    assert!(kept.file_name().unwrap().to_string_lossy().starts_with("state.json.corrupt-"));
    assert_eq!(fs::read_to_string(&kept).unwrap(), "{ \"theme\": ");
}

#[test]
fn a_value_of_the_wrong_type_is_treated_as_unreadable() {
    let dir = TempDir::new("wrong-type");
    write_state(dir.path(), r#"{ "settings": { "requestTimeoutSeconds": "soon" } }"#);

    let loaded = load(dir.path());

    assert_eq!(loaded.state, PersonalState::default());
    assert_eq!(loaded.issues.len(), 1);
}

#[test]
fn an_empty_file_is_treated_as_unreadable() {
    let dir = TempDir::new("empty-file");
    write_state(dir.path(), "");

    let loaded = load(dir.path());

    assert_eq!(loaded.state, PersonalState::default());
    assert_eq!(loaded.issues.len(), 1);
}

#[test]
fn a_reasonable_window_is_returned_and_an_unreasonable_one_is_not() {
    let dir = TempDir::new("window");
    write_state(dir.path(), r#"{ "window": { "x": 40, "y": 60, "width": 1200, "height": 800, "isMaximized": true } }"#);

    let state = load(dir.path()).state;
    let window = state.window().expect("a reasonable window is kept");
    assert_eq!((window.x, window.y, window.width, window.height, window.is_maximized), (40, 60, 1200.0, 800.0, true));

    write_state(dir.path(), r#"{ "window": { "x": 0, "y": 0, "width": 50, "height": 800 } }"#);
    assert_eq!(load(dir.path()).state.window(), None, "a window narrower than any real one is ignored");

    write_state(dir.path(), r#"{ "window": { "x": 25000, "y": 0, "width": 800, "height": 600 } }"#);
    assert_eq!(load(dir.path()).state.window(), None, "a position off every screen is ignored");
}

#[test]
fn panels_drop_only_the_damaged_value() {
    let dir = TempDir::new("panels");
    write_state(dir.path(), r#"{ "panels": { "sidebarWidth": 9000, "requestHeight": 320 } }"#);

    let panels = load(dir.path()).state.panels().expect("panels are saved");

    assert_eq!(panels.sidebar_width, None, "a sidebar wider than any screen falls back to the default");
    assert_eq!(panels.request_height, Some(320.0), "the other value is kept");
}

#[test]
fn panels_are_none_when_nothing_was_saved() {
    let dir = TempDir::new("no-panels");
    write_state(dir.path(), "{}");

    assert_eq!(load(dir.path()).state.panels(), None);
}

#[test]
fn key_value_splits_only_return_usable_shares() {
    let dir = TempDir::new("splits");
    write_state(dir.path(), r#"{ "keyValueSplits": { "headers": 0.4, "params": 1.0, "form": 0.0 } }"#);

    let state = load(dir.path()).state;

    assert_eq!(state.key_value_split("headers"), Some(0.4));
    assert_eq!(state.key_value_split("params"), None, "a share of 1 leaves no room for the value column");
    assert_eq!(state.key_value_split("form"), None, "a share of 0 is not usable");
    assert_eq!(state.key_value_split("missing"), None);
}

#[test]
fn resolve_uses_the_override_when_it_is_set() {
    let app_data = Path::new("C:/Users/someone/AppData/Roaming");

    assert_eq!(
        resolve_personal_directory(Some("D:/scratch/ricochet"), Some(app_data)),
        Some(PathBuf::from("D:/scratch/ricochet")),
    );
}

#[test]
fn resolve_ignores_an_empty_override() {
    let app_data = Path::new("C:/Users/someone/AppData/Roaming");

    assert_eq!(
        resolve_personal_directory(Some(""), Some(app_data)),
        Some(app_data.join("Ricochet")),
    );
}

#[test]
fn resolve_uses_the_app_data_folder_without_an_override() {
    let app_data = Path::new("/home/someone/.config");

    assert_eq!(resolve_personal_directory(None, Some(app_data)), Some(app_data.join("Ricochet")));
}

#[test]
fn resolve_gives_none_without_an_override_or_app_data() {
    assert_eq!(resolve_personal_directory(None, None), None);
}

#[test]
fn settings_become_the_sender_settings_the_c_sharp_app_configures() {
    let settings = ricochet_core::personal_state::PersonalSettings {
        request_timeout_seconds: 15,
        follow_redirects: false,
        verify_tls_certificates: false,
        max_response_megabytes: 2,
        wrap_response: true,
        proxy_url: "proxy.example.com:8080".to_string(),
        client_certificate_path: "C:/certs/me.pem".to_string(),
    };
    let sender = settings.sender_settings();
    assert_eq!(sender.timeout, std::time::Duration::from_secs(15));
    assert!(!sender.follow_redirects);
    assert!(!sender.verify_certificates);
    assert_eq!(sender.max_body_bytes, 2 * 1024 * 1024);
    assert_eq!(sender.proxy_url, "proxy.example.com:8080");
    assert_eq!(sender.client_certificate_path, "C:/certs/me.pem");
    assert!(sender.certificate_warning().is_some());
}

#[test]
fn a_timeout_or_size_below_one_falls_back_to_the_default() {
    let settings = ricochet_core::personal_state::PersonalSettings {
        request_timeout_seconds: 0,
        max_response_megabytes: -5,
        ..Default::default()
    };
    let sender = settings.sender_settings();
    assert_eq!(sender, ricochet_core::send::SenderSettings::default());
}

#[test]
fn saving_the_theme_changes_only_the_theme_key() {
    let dir = TempDir::new("theme");
    write_state(dir.path(), r#"{"activeEnvironmentId":"prod","futureKey":{"kept":true},"settings":{"requestTimeoutSeconds":5}}"#);

    save_theme(dir.path(), Some("light")).unwrap();

    let text = fs::read_to_string(dir.path().join(STATE_FILE_NAME)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["theme"], serde_json::json!("light"));
    assert_eq!(saved["activeEnvironmentId"], serde_json::json!("prod"));
    assert_eq!(saved["futureKey"], serde_json::json!({ "kept": true }));
    assert_eq!(saved["settings"], serde_json::json!({ "requestTimeoutSeconds": 5 }));
    assert_eq!(load(dir.path()).state.theme.as_deref(), Some("light"));
}

#[test]
fn saving_no_theme_writes_null_so_the_system_is_followed() {
    let dir = TempDir::new("theme-none");

    save_theme(dir.path(), Some("dark")).unwrap();
    save_theme(dir.path(), None).unwrap();

    let text = fs::read_to_string(dir.path().join(STATE_FILE_NAME)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["theme"], serde_json::Value::Null);
    assert_eq!(load(dir.path()).state.theme, None);
}

#[test]
fn saving_the_theme_leaves_a_file_that_is_not_an_object_alone() {
    let dir = TempDir::new("theme-bad");
    write_state(dir.path(), "[1, 2]");

    assert!(save_theme(dir.path(), Some("dark")).is_err());
    assert_eq!(fs::read_to_string(dir.path().join(STATE_FILE_NAME)).unwrap(), "[1, 2]");
}
