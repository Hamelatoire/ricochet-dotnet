//! Tests for saving the window placement, panel sizes and open tabs in state.json (ADR 0041, ADR 0043, ADR 0068),
//! keeping every other key.

use std::time::{SystemTime, UNIX_EPOCH};

use pretty_assertions::assert_eq;
use ricochet_core::personal_state::{PanelLayout, WindowPlacement, load, save_open_tabs, save_panels, save_window};

fn temp_dir(name: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("ricochet-memory-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn read(dir: &std::path::Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join("state.json")).unwrap()).unwrap()
}

const OTHER_KEYS: &str = r#"{"theme":"dark","activeEnvironmentId":"prod","settings":{"proxyUrl":"x"},"someFutureSetting":{"x":1}}"#;

fn assert_other_keys_kept(saved: &serde_json::Value) {
    assert_eq!(saved["theme"], "dark");
    assert_eq!(saved["activeEnvironmentId"], "prod");
    assert_eq!(saved["settings"], serde_json::json!({"proxyUrl": "x"}));
    assert_eq!(saved["someFutureSetting"], serde_json::json!({"x": 1}));
}

#[test]
fn a_saved_window_loads_back_and_keeps_other_keys() {
    let dir = temp_dir("window");
    std::fs::write(dir.join("state.json"), OTHER_KEYS).unwrap();
    let placement = WindowPlacement { x: 100, y: -40, width: 1200.0, height: 800.0, is_maximized: true };
    save_window(&dir, &placement).unwrap();

    let saved = read(&dir);
    assert_other_keys_kept(&saved);
    assert_eq!(saved["window"], serde_json::json!({"x": 100, "y": -40, "width": 1200.0, "height": 800.0, "isMaximized": true}));
    assert_eq!(load(&dir).state.window(), Some(&placement));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_window_no_screen_could_hold_is_not_saved_over_a_good_one() {
    let dir = temp_dir("bad-window");
    let good = WindowPlacement { x: 0, y: 0, width: 900.0, height: 700.0, is_maximized: false };
    save_window(&dir, &good).unwrap();
    save_window(&dir, &WindowPlacement { width: 50.0, ..good.clone() }).unwrap();
    save_window(&dir, &WindowPlacement { height: f64::NAN, ..good.clone() }).unwrap();
    assert_eq!(load(&dir).state.window(), Some(&good));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn panels_are_saved_with_damaged_values_dropped() {
    let dir = temp_dir("panels");
    std::fs::write(dir.join("state.json"), OTHER_KEYS).unwrap();
    save_panels(&dir, &PanelLayout { sidebar_width: Some(320.0), request_height: Some(f64::INFINITY) }).unwrap();

    let saved = read(&dir);
    assert_other_keys_kept(&saved);
    // A dropped value leaves no key, so it adds nothing to the file.
    assert_eq!(saved["panels"], serde_json::json!({"sidebarWidth": 320.0}));
    assert_eq!(load(&dir).state.panels(), Some(PanelLayout { sidebar_width: Some(320.0), request_height: None }));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn both_panel_sizes_load_back() {
    let dir = temp_dir("panels-both");
    let layout = PanelLayout { sidebar_width: Some(250.0), request_height: Some(300.0) };
    save_panels(&dir, &layout).unwrap();
    assert_eq!(load(&dir).state.panels(), Some(layout));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn open_tabs_and_the_active_tab_load_back_and_keep_other_keys() {
    let dir = temp_dir("tabs");
    std::fs::write(dir.join("state.json"), OTHER_KEYS).unwrap();
    save_open_tabs(&dir, &["a".to_string(), "b".to_string()], Some("b")).unwrap();

    let saved = read(&dir);
    assert_other_keys_kept(&saved);
    let state = load(&dir).state;
    assert_eq!(state.open_tabs(), vec!["a".to_string(), "b".to_string()]);
    assert_eq!(state.active_tab.as_deref(), Some("b"));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn closing_every_tab_removes_both_keys() {
    let dir = temp_dir("no-tabs");
    std::fs::write(dir.join("state.json"), OTHER_KEYS).unwrap();
    save_open_tabs(&dir, &["a".to_string()], Some("a")).unwrap();
    save_open_tabs(&dir, &[], Some("a")).unwrap();

    let saved = read(&dir);
    assert_other_keys_kept(&saved);
    assert!(saved.get("openTabs").is_none());
    assert!(saved.get("activeTab").is_none());
    assert_eq!(load(&dir).state.open_tabs(), Vec::<String>::new());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn saves_change_only_their_own_keys() {
    let dir = temp_dir("together");
    let placement = WindowPlacement { x: 10, y: 20, width: 800.0, height: 600.0, is_maximized: false };
    save_window(&dir, &placement).unwrap();
    save_panels(&dir, &PanelLayout { sidebar_width: Some(300.0), request_height: None }).unwrap();
    save_open_tabs(&dir, &["a".to_string()], None).unwrap();

    let state = load(&dir).state;
    assert_eq!(state.window(), Some(&placement));
    assert_eq!(state.panels().and_then(|panels| panels.sidebar_width), Some(300.0));
    assert_eq!(state.open_tabs(), vec!["a".to_string()]);
    assert_eq!(state.active_tab, None);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_file_that_is_not_an_object_is_left_alone() {
    let dir = temp_dir("not-object");
    std::fs::write(dir.join("state.json"), "[1,2]").unwrap();
    assert!(save_open_tabs(&dir, &["a".to_string()], None).is_err());
    assert_eq!(std::fs::read_to_string(dir.join("state.json")).unwrap(), "[1,2]");
    std::fs::remove_dir_all(dir).ok();
}
