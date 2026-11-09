use pretty_assertions::assert_eq;
use ricochet_core::models::*;
use serde_json::{Value, json};

#[test]
fn key_value_defaults_to_enabled_and_omits_false_flags() {
    let kv = KeyValue::new("Accept", "application/json");
    let text = serde_json::to_string(&kv).unwrap();
    assert_eq!(text, r#"{"key":"Accept","value":"application/json","enabled":true}"#);
}

#[test]
fn key_value_read_without_enabled_is_enabled() {
    let kv: KeyValue = serde_json::from_str(r#"{"key":"a","value":"b"}"#).unwrap();
    assert!(kv.enabled);
    assert!(!kv.is_secret);
    assert!(!kv.is_prompt);
}

#[test]
fn request_omits_empty_optional_fields() {
    let request = ApiRequest { name: "Ping".into(), url: "https://example.test".into(), ..Default::default() };
    let value: Value = serde_json::to_value(&request).unwrap();
    let object = value.as_object().unwrap();
    for absent in ["auth", "assertions", "captures", "specKey", "examples"] {
        assert!(!object.contains_key(absent), "{absent} should not be written when empty");
    }
    assert_eq!(object["method"], json!("GET"));
    assert_eq!(object["headers"], json!([]));
}

#[test]
fn request_reads_a_file_written_by_the_csharp_app() {
    let file = json!({
        "id": "3f2a",
        "name": "Get user",
        "method": "POST",
        "url": "https://api.test/users/{{id}}",
        "headers": [{ "key": "X-Trace", "value": "1", "enabled": false }],
        "body": "{}",
        "auth": { "kind": "bearer", "token": "" },
        "assertions": [{ "source": "status", "op": "equals", "expected": "200" }]
    });
    let request: ApiRequest = serde_json::from_value(file).unwrap();
    assert_eq!(request.method, "POST");
    assert!(!request.headers[0].enabled);
    assert_eq!(request.auth.unwrap().kind, auth_kinds::BEARER);
    assert_eq!(request.assertions.unwrap().len(), 1);
}

#[test]
fn auth_defaults_to_none_kind() {
    let auth: RequestAuth = serde_json::from_str("{}").unwrap();
    assert_eq!(auth.kind, auth_kinds::NONE);
    assert!(auth.details.is_none());
}

#[test]
fn collection_item_writes_folder_without_request() {
    let folder = CollectionItem { name: "Users".into(), ..Default::default() };
    let value = serde_json::to_value(&folder).unwrap();
    assert!(value.get("request").is_none());
    assert!(value.get("tags").is_none());
}

#[test]
fn workspace_round_trips_camel_case_fields() {
    let text = r#"{
        "collections": [],
        "environments": [{ "id": "e1", "name": "Staging", "variables": [{ "key": "host", "value": "s.test" }] }],
        "activeEnvironmentId": "e1",
        "globals": [],
        "history": []
    }"#;
    let workspace: Workspace = serde_json::from_str(text).unwrap();
    assert_eq!(workspace.active_environment_id.as_deref(), Some("e1"));
    assert_eq!(workspace.environments[0].name, "Staging");

    let written = serde_json::to_value(&workspace).unwrap();
    assert_eq!(written["activeEnvironmentId"], json!("e1"));
    assert!(written.get("saved").is_none());
    assert!(written.get("variables").is_none());
}

#[test]
fn history_entry_reads_timestamp_and_optional_flags() {
    let text = r#"{
        "request": { "id": "r1", "name": "x", "method": "GET", "url": "https://a.test", "headers": [], "body": "" },
        "statusCode": 204,
        "elapsedMs": 42,
        "at": "2026-11-09T12:00:00+00:00"
    }"#;
    let entry: HistoryEntry = serde_json::from_str(text).unwrap();
    assert_eq!(entry.status_code, Some(204));
    assert_eq!(entry.elapsed_ms, 42);
    assert!(entry.collection_id.is_none());
    assert!(entry.certificates_unverified.is_none());
}

#[test]
fn legacy_saved_requests_move_into_default_collection() {
    let text = r#"{
        "saved": [{ "id": "a", "name": "Old one", "method": "GET", "url": "https://old.test", "headers": [], "body": "" }],
        "variables": [{ "key": "token", "value": "t" }]
    }"#;
    let workspace: Workspace = serde_json::from_str(text).unwrap();
    let migrated = workspace.migrate_legacy_fields();

    assert_eq!(migrated.collections.len(), 1);
    assert_eq!(migrated.collections[0].name, DEFAULT_COLLECTION_NAME);
    assert_eq!(migrated.collections[0].items[0].name, "Old one");
    assert_eq!(migrated.globals[0].key, "token");

    let written = serde_json::to_value(&migrated).unwrap();
    assert!(written.get("saved").is_none());
    assert!(written.get("variables").is_none());
}

#[test]
fn migrating_a_current_workspace_changes_nothing() {
    let workspace = Workspace { collections: vec![Collection::default()], ..Default::default() };
    let migrated = workspace.clone().migrate_legacy_fields();
    assert_eq!(migrated, workspace);
}

#[test]
fn history_limit_matches_csharp() {
    assert_eq!(MAX_HISTORY, 200);
}
