use pretty_assertions::assert_eq;
use ricochet_app::edit::{VariableField, add_variable, remove_variable, set_variable_text, toggle_variable, variables_mut, variables_of};
use ricochet_core::models::{ApiRequest, Collection, CollectionItem, KeyValue, Workspace};

fn workspace() -> Workspace {
    let request = CollectionItem { id: "req".into(), name: "R".into(), request: Some(ApiRequest::default()), ..Default::default() };
    let folder = CollectionItem {
        id: "folder".into(),
        name: "F".into(),
        variables: vec![KeyValue::new("resource", "orders")],
        children: vec![request],
        ..Default::default()
    };
    Workspace {
        collections: vec![Collection { id: "c".into(), name: "C".into(), variables: vec![KeyValue::new("baseUrl", "http://a")], items: vec![folder], ..Default::default() }],
        ..Default::default()
    }
}

#[test]
fn a_collection_and_a_folder_expose_their_variables_but_a_request_does_not() {
    let mut workspace = workspace();
    assert_eq!(variables_of(&workspace, "c").unwrap()[0].key, "baseUrl");
    assert_eq!(variables_of(&workspace, "folder").unwrap()[0].key, "resource");
    assert!(variables_of(&workspace, "req").is_none());
    assert!(variables_mut(&mut workspace, "req").is_none());
    assert!(variables_mut(&mut workspace, "missing").is_none());
}

#[test]
fn names_and_values_are_set_and_unchanged_text_reports_no_change() {
    let mut workspace = workspace();
    let vars = variables_mut(&mut workspace, "folder").unwrap();
    assert!(set_variable_text(vars, 0, VariableField::Value, "shop"));
    assert!(!set_variable_text(vars, 0, VariableField::Value, "shop"));
    assert!(set_variable_text(vars, 0, VariableField::Name, "store"));
    assert!(!set_variable_text(vars, 5, VariableField::Name, "nope"));
    assert_eq!(variables_of(&workspace, "folder").unwrap()[0], KeyValue::new("store", "shop"));
}

#[test]
fn a_variable_can_be_added_toggled_and_removed() {
    let mut workspace = workspace();
    let vars = variables_mut(&mut workspace, "c").unwrap();
    let index = add_variable(vars);
    assert_eq!(index, 1);
    assert!(vars[index].enabled);
    assert!(toggle_variable(vars, index));
    assert!(!vars[index].enabled);
    assert!(remove_variable(vars, index));
    assert!(!remove_variable(vars, index));
    assert!(!toggle_variable(vars, index));
    assert_eq!(vars.len(), 1);
}
