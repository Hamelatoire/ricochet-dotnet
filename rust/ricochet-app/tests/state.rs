//! Tests for the application state: open tabs, the showing tab, the sidebar selection and the active Environment
//! (ADR 0041). Plain data, so no window is needed.

use pretty_assertions::assert_eq;
use ricochet_app::state::AppState;
use ricochet_core::models::{ApiEnvironment, Workspace};

fn open(ids: &[&str]) -> AppState {
    let mut state = AppState::default();
    for id in ids {
        state.open_tab(id);
    }
    state
}

#[test]
fn starts_with_no_tabs_and_nothing_selected() {
    let state = AppState::default();
    assert_eq!(state.open_tabs(), &[] as &[String]);
    assert_eq!(state.active_tab(), None);
    assert_eq!(state.selected_item(), None);
}

#[test]
fn takes_the_active_environment_from_the_workspace() {
    let workspace = Workspace {
        environments: vec![ApiEnvironment { id: "staging".into(), name: "Staging".into(), ..Default::default() }],
        active_environment_id: Some("staging".into()),
        ..Default::default()
    };
    assert_eq!(AppState::for_workspace(&workspace).active_environment_id(), Some("staging"));
    assert_eq!(AppState::for_workspace(&Workspace::default()).active_environment_id(), None);
}

#[test]
fn opening_a_tab_shows_and_selects_it_and_keeps_the_others() {
    let state = open(&["list-orders", "ping"]);
    assert_eq!(state.open_tabs(), &["list-orders".to_string(), "ping".to_string()]);
    assert_eq!(state.active_tab(), Some("ping"));
    assert_eq!(state.selected_item(), Some("ping"));
}

#[test]
fn opening_a_request_twice_keeps_one_tab_and_activates_it() {
    let state = open(&["list-orders", "ping", "list-orders"]);
    assert_eq!(state.open_tabs(), &["list-orders".to_string(), "ping".to_string()]);
    assert_eq!(state.active_tab(), Some("list-orders"));
}

#[test]
fn activating_an_open_tab_selects_its_request() {
    let mut state = open(&["list-orders", "ping"]);
    assert!(state.activate_tab("list-orders"));
    assert_eq!(state.active_tab(), Some("list-orders"));
    assert_eq!(state.selected_item(), Some("list-orders"));
    assert_eq!(state.open_tabs().len(), 2);
}

#[test]
fn activating_a_tab_that_is_not_open_changes_nothing() {
    let mut state = open(&["ping"]);
    assert!(!state.activate_tab("status"));
    assert_eq!(state.active_tab(), Some("ping"));
    assert_eq!(state.open_tabs(), &["ping".to_string()]);
}

#[test]
fn closing_a_background_tab_leaves_the_showing_tab_alone() {
    let mut state = open(&["list-orders", "ping", "status"]);
    assert!(state.activate_tab("status"));
    assert!(state.close_tab("list-orders"));
    assert_eq!(state.open_tabs(), &["ping".to_string(), "status".to_string()]);
    assert_eq!(state.active_tab(), Some("status"));
    assert_eq!(state.selected_item(), Some("status"));
}

#[test]
fn closing_the_showing_tab_moves_to_the_tab_that_takes_its_place() {
    let mut state = open(&["list-orders", "ping", "status"]);
    assert!(state.activate_tab("ping"));
    assert!(state.close_tab("ping"));
    assert_eq!(state.active_tab(), Some("status"));
    assert_eq!(state.selected_item(), Some("status"));
}

#[test]
fn closing_the_last_tab_moves_to_the_one_before_it() {
    let mut state = open(&["list-orders", "ping", "status"]);
    assert!(state.close_tab("status"));
    assert_eq!(state.active_tab(), Some("ping"));
    assert_eq!(state.selected_item(), Some("ping"));
}

#[test]
fn closing_the_only_tab_leaves_no_tab_showing() {
    let mut state = open(&["ping"]);
    assert!(state.close_tab("ping"));
    assert_eq!(state.open_tabs(), &[] as &[String]);
    assert_eq!(state.active_tab(), None);
    assert_eq!(state.selected_item(), None);
}

#[test]
fn closing_a_tab_that_is_not_open_changes_nothing() {
    let mut state = open(&["ping"]);
    assert!(!state.close_tab("status"));
    assert_eq!(state.open_tabs(), &["ping".to_string()]);
    assert_eq!(state.active_tab(), Some("ping"));
}

#[test]
fn selecting_a_folder_does_not_open_or_activate_a_tab() {
    let mut state = open(&["ping"]);
    state.select_item("orders");
    assert_eq!(state.selected_item(), Some("orders"));
    assert_eq!(state.active_tab(), Some("ping"));
    assert_eq!(state.open_tabs(), &["ping".to_string()]);
}

#[test]
fn picking_a_request_selects_it_and_opens_or_activates_its_tab() {
    let mut state = open(&["ping"]);
    state.pick_request("status");
    assert_eq!(state.selected_item(), Some("status"));
    assert_eq!(state.active_tab(), Some("status"));
    assert_eq!(state.open_tabs(), &["ping".to_string(), "status".to_string()]);

    state.pick_request("ping");
    assert_eq!(state.active_tab(), Some("ping"));
    assert_eq!(state.open_tabs().len(), 2);
}

#[test]
fn folding_an_item_is_a_toggle() {
    let mut state = AppState::default();
    assert!(!state.is_collapsed("orders"));
    state.toggle_collapsed("orders");
    assert!(state.is_collapsed("orders"));
    state.toggle_collapsed("orders");
    assert!(!state.is_collapsed("orders"));
}

#[test]
fn cycling_goes_through_each_environment_and_back_to_none() {
    let workspace = Workspace {
        environments: vec![
            ApiEnvironment { id: "staging".into(), name: "Staging".into(), ..Default::default() },
            ApiEnvironment { id: "prod".into(), name: "Production".into(), ..Default::default() },
        ],
        ..Default::default()
    };
    let mut state = AppState::for_workspace(&workspace);
    let mut seen = Vec::new();
    for _ in 0..4 {
        state.cycle_environment(&workspace);
        seen.push(state.active_environment_id().map(str::to_string));
    }
    assert_eq!(seen, vec![Some("staging".to_string()), Some("prod".to_string()), None, Some("staging".to_string())]);
}

#[test]
fn cycling_from_an_environment_that_is_gone_starts_over() {
    let workspace = Workspace {
        environments: vec![ApiEnvironment { id: "staging".into(), name: "Staging".into(), ..Default::default() }],
        active_environment_id: Some("deleted".into()),
        ..Default::default()
    };
    let mut state = AppState::for_workspace(&workspace);
    state.cycle_environment(&workspace);
    assert_eq!(state.active_environment_id(), Some("staging"));
}
