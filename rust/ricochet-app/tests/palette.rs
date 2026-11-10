use pretty_assertions::assert_eq;
use ricochet_app::palette::{PaletteAction, entries, filter};
use ricochet_core::models::{ApiEnvironment, ApiRequest, Collection, CollectionItem, Workspace};

fn workspace() -> Workspace {
    let status = CollectionItem { id: "status".into(), name: "Status".into(), request: Some(ApiRequest { name: "Status".into(), ..Default::default() }), ..Default::default() };
    let orders = CollectionItem { id: "orders".into(), name: "List orders".into(), request: Some(ApiRequest { name: "List orders".into(), ..Default::default() }), ..Default::default() };
    Workspace {
        collections: vec![Collection { id: "c".into(), name: "Shop".into(), items: vec![status, orders], ..Default::default() }],
        environments: vec![ApiEnvironment { id: "staging".into(), name: " Staging ".into(), variables: vec![] }],
        ..Default::default()
    }
}

#[test]
fn commands_then_environments_then_requests_are_offered() {
    let labels: Vec<String> = entries(&workspace()).into_iter().map(|entry| entry.label).collect();
    assert_eq!(
        labels,
        vec![
            "Send request",
            "Save",
            "Close all tabs",
            "Close other tabs",
            "New request",
            "Use no environment",
            "Use environment: Staging",
            "Open: Status",
            "Open: List orders",
        ]
    );
}

#[test]
fn a_request_entry_opens_that_request() {
    let found = entries(&workspace()).into_iter().find(|entry| entry.label == "Open: List orders").unwrap();
    assert_eq!(found.action, PaletteAction::OpenRequest("orders".to_string()));
}

#[test]
fn the_query_keeps_entries_with_every_word_ignoring_case() {
    let all = entries(&workspace());
    let shown: Vec<String> = filter(&all, "ORD").into_iter().map(|entry| entry.label).collect();
    assert_eq!(shown, vec!["Open: List orders"]);
    let shown: Vec<String> = filter(&all, "environment staging").into_iter().map(|entry| entry.label).collect();
    assert_eq!(shown, vec!["Use environment: Staging"]);
    assert_eq!(filter(&all, "   "), all);
    assert!(filter(&all, "nothing matches this").is_empty());
}
