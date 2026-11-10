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
