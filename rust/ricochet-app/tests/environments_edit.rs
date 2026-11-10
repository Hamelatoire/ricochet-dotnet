use pretty_assertions::assert_eq;
use ricochet_app::edit::{VariableField, delete_environment, new_environment, rename_environment, set_variable_text, variables_mut, variables_of};
use ricochet_core::models::{ApiEnvironment, KeyValue, Workspace};

fn workspace() -> Workspace {
    Workspace { environments: vec![ApiEnvironment { id: "staging".into(), name: "Staging".into(), variables: vec![KeyValue::new("host", "a")] }], ..Default::default() }
}

#[test]
fn a_new_environment_gets_a_unique_name_and_is_listed() {
    let mut workspace = workspace();
    let first = new_environment(&mut workspace, "New environment");
    let second = new_environment(&mut workspace, "New environment");
    assert_eq!(workspace.environments[1].name, "New environment");
    assert_eq!(workspace.environments[2].name, "New environment 2");
    assert_eq!(workspace.environments[1].id, first);
    assert_eq!(workspace.environments[2].id, second);
    assert!(variables_of(&workspace, &first).unwrap().is_empty());
}

#[test]
fn an_environment_is_renamed_and_its_variables_are_edited_like_a_folders() {
    let mut workspace = workspace();
    assert!(rename_environment(&mut workspace, "staging", "Prod"));
    assert!(!rename_environment(&mut workspace, "staging", "Prod"));
    assert!(!rename_environment(&mut workspace, "missing", "X"));
    assert_eq!(workspace.environments[0].name, "Prod");

    let vars = variables_mut(&mut workspace, "staging").unwrap();
    assert!(set_variable_text(vars, 0, VariableField::Value, "b"));
    assert_eq!(variables_of(&workspace, "staging").unwrap()[0].value, "b");
}

#[test]
fn an_environment_is_deleted_and_a_missing_one_is_not() {
    let mut workspace = workspace();
    assert!(delete_environment(&mut workspace, "staging"));
    assert!(!delete_environment(&mut workspace, "staging"));
    assert!(workspace.environments.is_empty());
    assert!(variables_of(&workspace, "staging").is_none());
}
