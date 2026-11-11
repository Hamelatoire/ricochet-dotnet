use pretty_assertions::assert_eq;
use ricochet_app::state::AppState;
use ricochet_core::models::{ApiRequest, Collection, CollectionItem, Workspace};

fn state_with_tabs(ids: &[&str]) -> AppState {
    let items = ids.iter().map(|id| CollectionItem { id: id.to_string(), name: id.to_string(), request: Some(ApiRequest { id: id.to_string(), ..Default::default() }), ..Default::default() }).collect();
    let workspace = Workspace { collections: vec![Collection { id: "c".into(), name: "C".into(), items, ..Default::default() }], ..Default::default() };
    let mut state = AppState::for_workspace(&workspace);
    for id in ids {
        state.open_tab(id);
    }
    state
}

#[test]
fn stepping_moves_through_the_tabs_and_wraps_round() {
    let mut state = state_with_tabs(&["a", "b", "c"]);
    assert_eq!(state.active_tab(), Some("c"));
    assert!(state.step_tab(1));
    assert_eq!(state.active_tab(), Some("a"));
    assert!(state.step_tab(-1));
    assert_eq!(state.active_tab(), Some("c"));
    assert_eq!(state.selected_item(), Some("c"));
}

#[test]
fn closing_the_showing_tab_moves_to_its_neighbour() {
    let mut state = state_with_tabs(&["a", "b", "c"]);
    state.activate_tab("b");
    assert!(state.close_active_tab());
    assert_eq!(state.open_tabs(), &["a".to_string(), "c".to_string()]);
    assert_eq!(state.active_tab(), Some("c"));
}

#[test]
fn close_all_and_close_other_leave_the_right_tabs() {
    let mut state = state_with_tabs(&["a", "b", "c"]);
    state.activate_tab("b");
    state.close_other_tabs();
    assert_eq!(state.open_tabs(), &["b".to_string()]);
    state.close_all_tabs();
    assert!(state.open_tabs().is_empty());
    assert_eq!(state.active_tab(), None);
    assert!(!state.step_tab(1));
    assert!(!state.close_active_tab());
}

fn workspace_with(ids: &[&str]) -> Workspace {
    let items = ids.iter().map(|id| CollectionItem { id: id.to_string(), name: id.to_string(), request: Some(ApiRequest { id: id.to_string(), ..Default::default() }), ..Default::default() }).collect();
    Workspace { collections: vec![Collection { id: "c".into(), name: "C".into(), items, ..Default::default() }], ..Default::default() }
}

fn ids(list: &[&str]) -> Vec<String> {
    list.iter().map(|id| id.to_string()).collect()
}

#[test]
fn restored_tabs_skip_requests_that_no_longer_exist_and_show_the_saved_active_tab() {
    let workspace = workspace_with(&["a", "b", "c"]);
    let mut state = AppState::for_workspace(&workspace);
    state.restore_tabs(&workspace, &ids(&["c", "gone", "a", "c"]), Some("a"));
    assert_eq!(state.open_tabs(), ids(&["c", "a"]).as_slice());
    assert_eq!(state.active_tab(), Some("a"));
    assert_eq!(state.selected_item(), Some("a"));
}

#[test]
fn a_missing_active_tab_falls_back_to_the_first() {
    let workspace = workspace_with(&["a", "b"]);
    let mut state = AppState::for_workspace(&workspace);
    state.restore_tabs(&workspace, &ids(&["b", "a"]), Some("gone"));
    assert_eq!(state.active_tab(), Some("b"));
}

#[test]
fn folders_and_collections_are_not_restored_as_tabs() {
    let workspace = workspace_with(&["a"]);
    let mut state = AppState::for_workspace(&workspace);
    state.restore_tabs(&workspace, &ids(&["c"]), Some("c"));
    assert!(state.open_tabs().is_empty());
    assert_eq!(state.active_tab(), None);
    assert_eq!(state.selected_item(), None);
}
