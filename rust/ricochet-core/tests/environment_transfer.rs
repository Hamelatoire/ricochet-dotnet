use pretty_assertions::assert_eq;
use ricochet_core::environment_transfer::*;
use ricochet_core::models::*;
use serde_json::{Value, json};

fn staging() -> ApiEnvironment {
    ApiEnvironment {
        id: "e1aaaaaaaa".into(),
        name: "Staging".into(),
        variables: vec![
            KeyValue::new("baseUrl", "https://staging.example.test"),
            KeyValue { key: "token".into(), value: "s3cret-value".into(), is_secret: true, ..KeyValue::default() },
            KeyValue { key: "off".into(), value: "1".into(), enabled: false, ..KeyValue::default() },
        ],
    }
}

fn named(name: &str) -> ApiEnvironment {
    ApiEnvironment { name: name.into(), ..ApiEnvironment::default() }
}

#[test]
fn export_leaves_secret_values_out_but_keeps_their_names() {
    let json = export(&staging());

    assert!(!json.contains("s3cret-value"), "secret value leaked into the export:\n{json}");
    let back = import(&json, "staging.json").unwrap().environment;
    assert_eq!(back.name, "Staging");
    let token = back.variables.iter().find(|v| v.key == "token").unwrap();
    assert!(token.is_secret);
    assert_eq!(token.value, "");
}

#[test]
fn export_omits_the_value_of_a_secret_rather_than_writing_null() {
    let value: Value = serde_json::from_str(&export(&staging())).unwrap();
    let token = value["variables"].as_array().unwrap().iter().find(|v| v["key"] == "token").unwrap();

    assert_eq!(token.as_object().unwrap().contains_key("value"), false);
    assert_eq!(token["isSecret"], json!(true));
}

#[test]
fn export_writes_the_format_header_and_indents_like_the_csharp_app() {
    let value: Value = serde_json::from_str(&export(&staging())).unwrap();

    assert_eq!(value["format"], json!("ricochet-environment"));
    assert_eq!(value["version"], json!(1));
    assert!(export(&staging()).contains("\n  \"name\": \"Staging\""));
}

#[test]
fn export_keeps_plain_values_and_enabled_flags() {
    let back = import(&export(&staging()), "staging.json").unwrap().environment;

    assert_eq!(back.variables.iter().find(|v| v.key == "baseUrl").unwrap().value, "https://staging.example.test");
    assert!(!back.variables.iter().find(|v| v.key == "off").unwrap().enabled);
    assert!(!back.variables.iter().find(|v| v.key == "baseUrl").unwrap().is_secret);
}

#[test]
fn import_of_an_export_reports_each_secret_as_needing_a_value() {
    let result = import(&export(&staging()), "staging.json").unwrap();

    assert_eq!(result.issues.len(), 1);
    let issue = &result.issues[0];
    assert_eq!(issue.category, SECRETS);
    assert!(issue.detail.contains("token"), "{}", issue.detail);
}

#[test]
fn import_summary_counts_variables() {
    let result = import(&export(&staging()), "staging.json").unwrap();

    assert_eq!(result.summary, "Imported environment \"Staging\" with 3 variables.");
}

#[test]
fn a_secret_that_arrives_with_a_value_keeps_it_for_the_vault() {
    let json = r#"{ "format": "ricochet-environment", "version": 1, "name": "Dev",
        "variables": [ { "key": "pw", "value": "hunter2", "enabled": true, "isSecret": true } ] }"#;

    let variable = import(json, "dev.json").unwrap().environment.variables.into_iter().next().unwrap();

    assert!(variable.is_secret);
    assert_eq!(variable.value, "hunter2");
}

#[test]
fn a_file_without_a_name_takes_it_from_the_file_name() {
    let json = r#"{ "format": "ricochet-environment", "variables": [] }"#;

    let result = import(json, "Production.json").unwrap();

    assert_eq!(result.environment.name, "Production");
}

#[test]
fn variable_names_are_fitted_and_reported() {
    let json = r#"{ "format": "ricochet-environment", "name": "Dev",
        "variables": [ { "key": "my token!", "value": "1" }, { "key": "n", "value": "42" } ] }"#;

    let result = import(json, "dev.json").unwrap();

    let keys: Vec<&str> = result.environment.variables.iter().map(|v| v.key.as_str()).collect();
    assert_eq!(keys, ["my_token_", "n"]);
    assert_eq!(result.environment.variables[1].value, "42");
    assert_eq!(result.issues.len(), 1);
    assert_eq!(result.issues[0].category, RENAMED_VARIABLES);
    assert!(result.issues[0].detail.contains("my_token_"), "{}", result.issues[0].detail);
}

#[test]
fn variables_with_empty_names_are_dropped() {
    let json = r#"{ "format": "ricochet-environment", "name": "Dev",
        "variables": [ { "key": "", "value": "x" }, { "key": "kept", "value": "y" } ] }"#;

    let result = import(json, "dev.json").unwrap();

    let keys: Vec<&str> = result.environment.variables.iter().map(|v| v.key.as_str()).collect();
    assert_eq!(keys, ["kept"]);
}

#[test]
fn collections_and_globals_are_not_environments() {
    let collection = r#"{ "info": { "name": "Shop", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" }, "item": [] }"#;
    let globals = r#"{ "_postman_variable_scope": "globals", "values": [ { "key": "g", "value": "1", "enabled": true } ] }"#;

    assert!(import(collection, "shop.json").is_err());
    assert!(import(globals, "globals.json").is_err());
}

#[test]
fn newer_format_versions_are_refused() {
    let json = r#"{ "format": "ricochet-environment", "version": 99, "name": "X", "variables": [] }"#;

    let error = import(json, "x.json").unwrap_err();

    assert!(error.message.contains("newer than this app reads"), "{}", error.message);
}

#[test]
fn invalid_json_is_refused() {
    let error = import("{ nope", "x.json").unwrap_err();

    assert!(error.message.starts_with("Not valid JSON"), "{}", error.message);
}

#[test]
fn without_a_clash_the_environment_is_added() {
    let placed = place(&[staging()], named("Prod"), EnvironmentClash::Skip);

    let names: Vec<&str> = placed.environments.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["Staging", "Prod"]);
    assert!(placed.placed.is_some());
}

#[test]
fn secret_count_counts_secret_variables_whether_or_not_they_have_a_value() {
    let environment = ApiEnvironment {
        name: "Staging".into(),
        variables: vec![
            KeyValue::new("baseUrl", "x"),
            KeyValue { key: "token".into(), value: "s3cret-value".into(), is_secret: true, ..KeyValue::default() },
            KeyValue { key: "empty".into(), is_secret: true, ..KeyValue::default() },
        ],
        ..ApiEnvironment::default()
    };

    assert_eq!(secret_count(&environment), 2);
}

#[test]
fn find_matches_a_name_ignoring_case_and_returns_none_when_absent_or_ambiguous() {
    let environments = vec![staging(), named("Prod")];

    assert_eq!(find_environment(&environments, "staging").map(|e| e.name.as_str()), Some("Staging"));
    assert!(find_environment(&environments, "Dev").is_none());
    assert!(find_environment(&[staging(), named("STAGING")], "staging").is_none());
}

#[test]
fn replace_overwrites_and_keeps_the_existing_id() {
    let incoming = ApiEnvironment {
        name: "staging".into(),
        variables: vec![KeyValue::new("baseUrl", "https://new.example.test")],
        ..ApiEnvironment::default()
    };

    let placed = place(&[staging()], incoming, EnvironmentClash::Replace);

    assert_eq!(placed.environments.len(), 1);
    let only = &placed.environments[0];
    assert_eq!(only.id, "e1aaaaaaaa");
    assert_eq!(only.variables[0].value, "https://new.example.test");
}

#[test]
fn replace_keeps_this_machines_secret_values_when_the_file_has_none() {
    let incoming = ApiEnvironment {
        name: "Staging".into(),
        variables: vec![
            KeyValue { key: "token".into(), is_secret: true, ..KeyValue::default() },
            KeyValue { key: "fresh".into(), is_secret: true, ..KeyValue::default() },
        ],
        ..ApiEnvironment::default()
    };

    let placed = place(&[staging()], incoming, EnvironmentClash::Replace);

    let variables = &placed.environments[0].variables;
    assert_eq!(variables.iter().find(|v| v.key == "token").unwrap().value, "s3cret-value");
    assert_eq!(variables.iter().find(|v| v.key == "fresh").unwrap().value, "");
}

#[test]
fn copy_numbers_the_name() {
    let placed = place(&[staging()], named("Staging"), EnvironmentClash::Copy);

    let names: Vec<&str> = placed.environments.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["Staging", "Staging (2)"]);
    assert_ne!(placed.placed.unwrap().id, "e1aaaaaaaa");
}

#[test]
fn copy_skips_numbers_already_taken() {
    let existing = vec![staging(), named("Staging (2)")];

    let placed = place(&existing, named("Staging"), EnvironmentClash::Copy);

    assert_eq!(placed.placed.unwrap().name, "Staging (3)");
}

#[test]
fn skip_leaves_the_existing_environment_alone() {
    let existing = vec![staging()];

    let placed = place(&existing, named("Staging"), EnvironmentClash::Skip);

    assert!(placed.placed.is_none());
    assert_eq!(placed.environments, existing);
}

#[test]
fn export_does_not_leak_a_secret_value_into_the_text_even_when_its_name_matches_a_plain_variable() {
    let environment = ApiEnvironment {
        name: "Dev".into(),
        variables: vec![
            KeyValue { key: "apiKey".into(), value: "postman-secret-99".into(), is_secret: true, ..KeyValue::default() },
            KeyValue::new("host", "api.example.test"),
        ],
        ..ApiEnvironment::default()
    };

    let json = export(&environment);

    assert!(!json.contains("postman-secret-99"));
    assert!(json.contains("api.example.test"));
}
