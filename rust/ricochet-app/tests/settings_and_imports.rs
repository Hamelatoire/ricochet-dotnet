//! Ports the Settings page and import cases of `MainViewModelTests.cs` (ADR 0002, 0007, 0014, 0038, 0069): typed
//! settings are checked before they apply, search narrows the sections, and imports add items under free names,
//! merge globals, keep a bad file from stopping the others, import only the ticked dump entries, and update an
//! OpenAPI Collection from the same address instead of adding it again.

use std::collections::HashSet;

use pretty_assertions::assert_eq;
use ricochet_app::import_page::{self, apply_dump, apply_spec, import_text, preview_spec};
use ricochet_app::settings_page::{TextSetting, apply_text, text_of, visible_sections};
use ricochet_core::models::{Collection, KeyValue, Workspace};
use ricochet_core::personal_state::PersonalSettings;
use ricochet_core::postman::{self, DumpEntry, EntryKind};

const V21: &str = "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";

fn collection_export(name: &str) -> String {
    format!(r#"{{ "info": {{ "name": "{name}", "schema": "{V21}" }}, "item": [ {{ "name": "List", "request": {{ "method": "GET", "url": "https://x.test/items" }} }} ] }}"#)
}

const ENVIRONMENT: &str = r#"{ "name": "Staging", "values": [ { "key": "host", "value": "x.test", "enabled": true } ] }"#;
const GLOBALS: &str = r#"{ "_postman_variable_scope": "globals", "values": [ { "key": "token", "value": "new", "enabled": true } ] }"#;

const SPEC: &str = r#"{ "openapi": "3.0.3", "info": { "title": "Pets" },
  "servers": [ { "url": "https://api.example.test" } ],
  "paths": { "/pets": { "get": { "summary": "List pets" } } } }"#;

// ---- settings ----

#[test]
fn a_typed_timeout_applies_only_when_it_is_a_whole_number_in_range() {
    let mut settings = PersonalSettings::default();
    assert_eq!(apply_text(&mut settings, TextSetting::Timeout, " 30 "), Ok(()));
    assert_eq!(settings.request_timeout_seconds, 30);

    for bad in ["0", "3601", "abc", ""] {
        let error = apply_text(&mut settings, TextSetting::Timeout, bad).unwrap_err();
        assert_eq!(error, "The timeout must be a whole number between 1 and 3600.");
        assert_eq!(settings.request_timeout_seconds, 30, "a bad value leaves the setting as it was");
    }
}

#[test]
fn text_fields_start_with_the_current_values_and_trim_what_is_typed() {
    let mut settings = PersonalSettings::default();
    assert_eq!(text_of(&settings, TextSetting::MaxResponse), "100");
    apply_text(&mut settings, TextSetting::Proxy, "  proxy.test:8080 ").unwrap();
    apply_text(&mut settings, TextSetting::ClientCertificate, " C:/me.pem").unwrap();
    assert_eq!(settings.proxy_url, "proxy.test:8080");
    assert_eq!(text_of(&settings, TextSetting::ClientCertificate), "C:/me.pem");
    assert!(apply_text(&mut settings, TextSetting::MaxResponse, "-1").is_err());
}

#[test]
fn search_shows_only_sections_with_a_matching_setting() {
    let all = visible_sections("");
    assert_eq!(all.iter().map(|(s, _)| s.title).collect::<Vec<_>>(), ["Requests", "Network", "Responses", "Privacy", "Workspace"]);

    let proxy = visible_sections("proxy");
    assert_eq!(proxy.len(), 1);
    assert_eq!(proxy[0].0.title, "Network");
    assert_eq!(proxy[0].1.iter().map(|i| i.key).collect::<Vec<_>>(), ["proxy-address"]);

    assert!(visible_sections("nothing matches this").is_empty());
}

// ---- imports ----

#[test]
fn an_import_with_a_taken_name_is_added_under_a_numbered_name() {
    let mut workspace = Workspace { collections: vec![Collection { name: "Shop".into(), ..Collection::default() }], ..Workspace::default() };
    let applied = import_text(&mut workspace, &collection_export("shop"), "shop.json");

    assert_eq!(workspace.collections.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Shop", "shop (2)"]);
    assert!(applied.report.starts_with("shop.json\n"));
    assert!(applied.report.contains("added as \"shop (2)\""), "{}", applied.report);
    assert_eq!(applied.changed_ids, vec![workspace.collections[1].id.clone()]);
}

#[test]
fn imported_globals_replace_same_names_and_are_reported() {
    let mut workspace = Workspace { globals: vec![KeyValue::new("token", "old")], ..Workspace::default() };
    let applied = import_text(&mut workspace, GLOBALS, "globals.json");

    assert!(applied.globals_changed);
    assert_eq!(workspace.globals[0].value, "new");
    assert!(applied.report.contains("Replaced existing values of: token"), "{}", applied.report);
}

#[test]
fn a_file_that_is_not_an_export_is_reported_and_changes_nothing() {
    let mut workspace = Workspace::default();
    let applied = import_text(&mut workspace, "{ \"hello\": 1 }", "notes.json");
    assert!(applied.report.starts_with("notes.json\nNot imported."), "{}", applied.report);
    assert!(applied.changed_ids.is_empty());
    assert_eq!(workspace, Workspace::default());
}

#[test]
fn only_the_ticked_dump_entries_are_imported() {
    let entry = |file: &str, name: &str, kind, json: &str| DumpEntry {
        file_name: file.into(),
        name: name.into(),
        kind,
        result: postman::import(json, file).unwrap(),
    };
    let listing = vec![
        entry("shop.json", "Shop", EntryKind::Collection, &collection_export("Shop")),
        entry("staging.json", "Staging", EntryKind::Environment, ENVIRONMENT),
    ];
    let mut workspace = Workspace::default();

    let ticked: HashSet<String> = ["staging.json".to_string()].into();
    let applied = apply_dump(&mut workspace, "dump.zip", &listing, &ticked);
    assert!(workspace.collections.is_empty());
    assert_eq!(workspace.environments[0].name, "Staging");
    assert!(applied.report.starts_with("dump.zip\n\n"));

    let nothing = apply_dump(&mut workspace, "dump.zip", &listing, &HashSet::new());
    assert_eq!(nothing.report, "dump.zip\nNothing was selected, so nothing was imported.");
}

#[test]
fn a_data_dump_is_recognised_by_its_extension() {
    assert!(import_page::is_dump(std::path::Path::new("C:/exports/Backup.ZIP")));
    assert!(!import_page::is_dump(std::path::Path::new("shop.json")));
}

#[test]
fn an_openapi_import_is_added_then_updated_from_the_same_address() {
    let address = "https://api.example.test/openapi.json";
    let mut workspace = Workspace::default();

    let first = preview_spec(&workspace, SPEC, address).unwrap();
    assert!(first.summary.is_new);
    let applied = apply_spec(&mut workspace, first);
    assert!(applied.report.starts_with(&format!("From {address}\n\n")));
    assert_eq!(workspace.collections.len(), 1);

    let second = preview_spec(&workspace, SPEC, address).unwrap();
    assert!(!second.summary.is_new);
    apply_spec(&mut workspace, second);
    assert_eq!(workspace.collections.len(), 1, "the update replaces the Collection instead of adding one");

    assert!(preview_spec(&workspace, "not json", address).is_err());
}
