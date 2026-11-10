use pretty_assertions::assert_eq;
use ricochet_app::edit::{collection_id_of, collection_variables, set_override};
use ricochet_core::models::{ApiEnvironment, ApiRequest, Collection, CollectionItem, KeyValue, Workspace};

fn workspace() -> Workspace {
    let request = CollectionItem { id: "req".into(), name: "R".into(), request: Some(ApiRequest::default()), ..Default::default() };
    let folder = CollectionItem { id: "folder".into(), name: "F".into(), children: vec![request], ..Default::default() };
    Workspace {
        collections: vec![Collection { id: "c".into(), name: "C".into(), variables: vec![KeyValue::new("host", "default.test")], items: vec![folder], ..Default::default() }],
        environments: vec![ApiEnvironment { id: "staging".into(), name: "Staging".into(), variables: vec![] }],
        ..Default::default()
    }
}

#[test]
fn the_collection_is_found_from_any_item_inside_it() {
    let workspace = workspace();
    assert_eq!(collection_id_of(&workspace, "req").as_deref(), Some("c"));
    assert_eq!(collection_id_of(&workspace, "c").as_deref(), Some("c"));
    assert_eq!(collection_id_of(&workspace, "missing"), None);
    assert_eq!(collection_variables(&workspace, "c")[0].value, "default.test");
}

#[test]
fn a_typed_value_overrides_the_default_and_an_empty_one_falls_back() {
    let mut workspace = workspace();
    assert!(set_override(&mut workspace, "staging", "host", "staging.test"));
    assert!(!set_override(&mut workspace, "staging", "host", "staging.test"));
    assert_eq!(workspace.environments[0].variables, vec![KeyValue::new("host", "staging.test")]);

    assert!(set_override(&mut workspace, "staging", "host", ""));
    assert!(workspace.environments[0].variables.is_empty());
    assert!(!set_override(&mut workspace, "staging", "host", ""));
    assert!(!set_override(&mut workspace, "missing", "host", "x"));
}
