//! Tests for filtering the sidebar tree by name.

use pretty_assertions::assert_eq;
use ricochet_app::tree::{TreeRow, filter_rows};
use ricochet_core::models::{ApiRequest, Collection, CollectionItem, Workspace};
use ricochet_app::tree::build_tree;

fn request(id: &str, name: &str) -> CollectionItem {
    CollectionItem { id: id.into(), name: name.into(), request: Some(ApiRequest { id: id.into(), name: name.into(), ..Default::default() }), ..Default::default() }
}

fn folder(id: &str, name: &str, children: Vec<CollectionItem>) -> CollectionItem {
    CollectionItem { id: id.into(), name: name.into(), children, ..Default::default() }
}

fn workspace() -> Workspace {
    let orders = folder("orders", "Orders", vec![request("list", "List orders"), request("create", "Create order")]);
    let users = folder("users", "Users", vec![request("me", "Current user")]);
    Workspace {
        collections: vec![
            Collection { id: "shop".into(), name: "Shop API".into(), items: vec![request("status", "Status"), orders, users], ..Default::default() },
            Collection { id: "other".into(), name: "Other".into(), items: vec![request("ping", "Ping")], ..Default::default() },
        ],
        ..Default::default()
    }
}

fn ids(rows: &[TreeRow]) -> Vec<&str> {
    rows.iter().map(|row| row.id.as_str()).collect()
}

#[test]
fn an_empty_query_keeps_every_row() {
    let rows = build_tree(&workspace(), false);
    assert_eq!(filter_rows(&rows, "   ").len(), rows.len());
}

#[test]
fn a_match_keeps_its_folder_and_collection_and_nothing_else() {
    let rows = build_tree(&workspace(), false);
    assert_eq!(ids(&filter_rows(&rows, "ORDER")), vec!["shop", "orders", "list", "create"]);
}

#[test]
fn a_match_in_a_collection_name_only_keeps_the_collection_row_and_its_matches() {
    let rows = build_tree(&workspace(), false);
    assert_eq!(ids(&filter_rows(&rows, "ping")), vec!["other", "ping"]);
    assert_eq!(filter_rows(&rows, "shop").len(), 1, "a collection with no matching request shows only itself");
}

#[test]
fn nothing_matching_gives_no_rows() {
    let rows = build_tree(&workspace(), false);
    assert!(filter_rows(&rows, "zzz").is_empty());
}
