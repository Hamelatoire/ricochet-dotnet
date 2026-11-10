//! Tests for the Variables a send can see (ADR 0009, ADR 0020).

use pretty_assertions::assert_eq;
use ricochet_app::scope::{collection_of, variables_in_scope};
use ricochet_core::models::{ApiEnvironment, ApiRequest, Collection, CollectionItem, KeyValue, Workspace};
use ricochet_core::variables::apply;

fn request_item(id: &str, url: &str) -> CollectionItem {
    CollectionItem {
        id: id.into(),
        name: id.into(),
        request: Some(ApiRequest { id: id.into(), url: url.into(), ..Default::default() }),
        ..Default::default()
    }
}

fn workspace() -> Workspace {
    let folder = CollectionItem {
        id: "folder".into(),
        name: "Orders".into(),
        variables: vec![KeyValue::new("host", "folder.test")],
        children: vec![request_item("inside", "https://{{host}}/{{path}}")],
        ..Default::default()
    };
    Workspace {
        globals: vec![KeyValue::new("host", "global.test"), KeyValue::new("path", "from-global")],
        collections: vec![Collection {
            id: "shop".into(),
            name: "Shop".into(),
            variables: vec![KeyValue::new("host", "collection.test")],
            items: vec![folder, request_item("outside", "https://{{host}}/{{path}}")],
            ..Default::default()
        }],
        environments: vec![ApiEnvironment {
            id: "staging".into(),
            name: "Staging".into(),
            variables: vec![KeyValue::new("path", "from-environment")],
        }],
        ..Default::default()
    }
}

fn sent_url(workspace: &Workspace, id: &str, environment: Option<&str>, dot_env: &[KeyValue]) -> String {
    let request = ricochet_app::tree::find_request(workspace, id).unwrap();
    apply(request, &variables_in_scope(workspace, id, environment, dot_env)).url
}

#[test]
fn finds_the_collection_of_a_nested_request() {
    let workspace = workspace();
    assert_eq!(collection_of(&workspace, "inside").map(|c| c.id.as_str()), Some("shop"));
    assert_eq!(collection_of(&workspace, "missing").map(|c| c.id.as_str()), None);
}

#[test]
fn a_folder_overrides_its_collection_and_the_globals() {
    assert_eq!(sent_url(&workspace(), "inside", None, &[]), "https://folder.test/from-global");
}

#[test]
fn a_request_outside_the_folder_does_not_see_its_variables() {
    assert_eq!(sent_url(&workspace(), "outside", None, &[]), "https://collection.test/from-global");
}

#[test]
fn the_active_environment_overrides_folders_and_dot_env_overrides_all() {
    let workspace = workspace();
    assert_eq!(sent_url(&workspace, "inside", Some("staging"), &[]), "https://folder.test/from-environment");
    let dot_env = [KeyValue::new("host", "local.test")];
    assert_eq!(sent_url(&workspace, "inside", Some("staging"), &dot_env), "https://local.test/from-environment");
}

#[test]
fn an_unknown_environment_adds_nothing() {
    assert_eq!(sent_url(&workspace(), "outside", Some("gone"), &[]), "https://collection.test/from-global");
}

#[test]
fn a_folded_folder_keeps_its_row_and_hides_what_is_inside() {
    use ricochet_app::tree::{build_tree, build_tree_folded};
    use std::collections::BTreeSet;

    let workspace = workspace();
    let all: Vec<String> = build_tree(&workspace, false).into_iter().map(|row| row.id).collect();
    assert_eq!(all, vec!["shop", "folder", "inside", "outside"]);

    let folded = BTreeSet::from(["folder".to_string()]);
    let rows: Vec<String> = build_tree_folded(&workspace, false, &folded).into_iter().map(|row| row.id).collect();
    assert_eq!(rows, vec!["shop", "folder", "outside"]);

    let folded = BTreeSet::from(["shop".to_string()]);
    let rows: Vec<String> = build_tree_folded(&workspace, false, &folded).into_iter().map(|row| row.id).collect();
    assert_eq!(rows, vec!["shop"]);
}
