//! Port of `InsomniaImporterTests.cs`. All fixtures are made up.

use pretty_assertions::assert_eq;
use ricochet_core::insomnia::{self, ImportCategories, InsomniaImportResult, InsomniaWorkspaceImport};
use ricochet_core::models::{auth_kinds, ApiRequest, CollectionItem};

// A made-up v4 export: one workspace with a base environment, a sub-environment, a folder and loose requests.
const EXPORT: &str = r#"{
  "_type": "export",
  "__export_format": 4,
  "__export_source": "insomnia.desktop.app:v0.0.0",
  "resources": [
    { "_id": "wrk_1", "_type": "workspace", "name": "Shop", "description": "", "metaSortKey": 1 },
    { "_id": "env_base", "_type": "environment", "parentId": "wrk_1", "name": "Base Environment",
      "data": { "base_url": "https://api.example.test", "page_size": 20 }, "metaSortKey": 2 },
    { "_id": "env_dev", "_type": "environment", "parentId": "env_base", "name": "Dev",
      "data": { "base_url": "https://dev.example.test" }, "metaSortKey": 3 },
    { "_id": "fld_orders", "_type": "request_group", "parentId": "wrk_1", "name": "Orders", "metaSortKey": 5,
      "headers": [ { "name": "X-Client", "value": "test-client" } ],
      "authentication": { "type": "bearer", "token": "{{ _.api_token }}" } },
    { "_id": "req_list", "_type": "request", "parentId": "fld_orders", "name": "List orders", "metaSortKey": 6,
      "method": "GET", "url": "{{ _.base_url }}/orders", "headers": [], "parameters": [
        { "name": "page", "value": "1" },
        { "name": "debug", "value": "1", "disabled": true } ],
      "authentication": { "type": "inherit" } },
    { "_id": "req_create", "_type": "request", "parentId": "fld_orders", "name": "Create order", "metaSortKey": 7,
      "method": "POST", "url": "{{ _.base_url }}/orders", "description": "Makes an order",
      "headers": [ { "name": "X-Client", "value": "request-client" }, { "name": "X-Trace", "value": "off", "disabled": true } ],
      "body": { "mimeType": "application/json", "text": "{\"sku\": \"{% uuid 'v4' %}\"}" },
      "authentication": { "type": "none" } },
    { "_id": "req_root", "_type": "request", "parentId": "wrk_1", "name": "Ping", "metaSortKey": 8,
      "method": "get", "url": "{{ _.base_url }}/ping",
      "authentication": { "type": "basic", "username": "tester", "password": "example-password" } },
    { "_id": "grpc_1", "_type": "grpc_request", "parentId": "wrk_1", "name": "Stream", "metaSortKey": 9 }
  ]
}"#;

fn import_export() -> InsomniaImportResult {
    insomnia::import(EXPORT, "shop.json").expect("fixture imports")
}

fn only_workspace(result: &InsomniaImportResult) -> &InsomniaWorkspaceImport {
    assert_eq!(result.workspaces.len(), 1);
    &result.workspaces[0]
}

fn flatten(item: &CollectionItem) -> Vec<&CollectionItem> {
    let mut all = vec![item];
    all.extend(item.children.iter().flat_map(flatten));
    all
}

fn find_request<'a>(items: &'a [CollectionItem], name: &str) -> &'a ApiRequest {
    items
        .iter()
        .flat_map(flatten)
        .find(|i| i.name == name)
        .and_then(|i| i.request.as_ref())
        .unwrap_or_else(|| panic!("no request named {name}"))
}

#[test]
fn imports_workspace_as_collection_with_folders_and_requests_in_sort_order() {
    let result = import_export();
    let workspace = only_workspace(&result);
    let items = &workspace.collection.items;

    assert_eq!(workspace.collection.name, "Shop");
    assert_eq!(items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["Orders", "Ping"]);
    assert_eq!(items[0].children.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["List orders", "Create order"]);
    assert_eq!(find_request(items, "Create order").method, "POST");
    assert_eq!(find_request(items, "Ping").method, "GET");
    assert!(result.summary.contains("3 requests, 1 folder"), "summary was {}", result.summary);
}

#[test]
fn unsupported_resource_types_are_reported_not_imported() {
    let result = import_export();
    let items = &only_workspace(&result).collection.items;

    assert_eq!(items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["Orders", "Ping"]);
    assert!(result.issues.iter().any(|i| {
        i.category == ImportCategories::DROPPED_DATA && i.location == "Shop / Stream" && i.detail == "grpc_request not imported"
    }));
}

#[test]
fn base_environment_becomes_collection_variables_and_sub_environments_become_environments() {
    let workspace = only_workspace(&import_export()).clone();

    let keys: Vec<&str> = workspace.collection.variables.iter().map(|v| v.key.as_str()).collect();
    assert_eq!(keys, ["base_url", "page_size"]);
    let page_size = workspace.collection.variables.iter().find(|v| v.key == "page_size").unwrap();
    assert_eq!(page_size.value, "20");

    assert_eq!(workspace.environments.len(), 1);
    let environment = &workspace.environments[0];
    assert_eq!(environment.name, "Dev");
    assert_eq!(environment.variables.len(), 1);
    assert_eq!(environment.variables[0].value, "https://dev.example.test");
}

#[test]
fn insomnia_placeholders_are_rewritten_to_ours() {
    let result = import_export();
    let list = find_request(&only_workspace(&result).collection.items, "List orders");

    assert_eq!(list.url, "{{base_url}}/orders");
    assert!(!list.url.contains("{{ _."));
}

#[test]
fn folder_headers_apply_unless_the_request_sets_the_same_name() {
    let result = import_export();
    let items = &only_workspace(&result).collection.items;

    let list = find_request(items, "List orders");
    let pairs: Vec<(&str, &str)> = list.headers.iter().map(|h| (h.key.as_str(), h.value.as_str())).collect();
    assert_eq!(pairs, [("X-Client", "test-client")]);

    let create = find_request(items, "Create order");
    let client_headers: Vec<_> = create.headers.iter().filter(|h| h.enabled && h.key == "X-Client").collect();
    assert_eq!(client_headers.len(), 1);
    assert_eq!(client_headers[0].value, "request-client");
    assert!(create.headers.iter().any(|h| h.key == "X-Trace" && !h.enabled));
}

#[test]
fn bearer_is_inherited_from_folder_and_resolved_onto_the_request() {
    let result = import_export();
    let list = find_request(&only_workspace(&result).collection.items, "List orders");
    let auth = list.auth.as_ref().expect("inherits bearer");

    assert_eq!(auth.kind, auth_kinds::BEARER);
    assert_eq!(auth.token, "{{api_token}}");
}

#[test]
fn basic_auth_is_imported_and_none_removes_inherited_auth() {
    let result = import_export();
    let items = &only_workspace(&result).collection.items;

    let ping = find_request(items, "Ping").auth.as_ref().expect("basic auth");
    assert_eq!(ping.kind, auth_kinds::BASIC);
    assert_eq!(ping.username, "tester");
    assert_eq!(ping.password, "example-password");
    assert!(find_request(items, "Create order").auth.is_none());
}

#[test]
fn disabled_query_parameters_are_reported() {
    let result = import_export();

    assert!(result.issues.iter().any(|i| {
        i.category == ImportCategories::DROPPED_DATA
            && i.location == "Shop / Orders / List orders"
            && i.detail == "disabled query parameter \"debug\""
    }));
}

#[test]
fn body_is_kept_with_its_content_type() {
    let result = import_export();
    let create = find_request(&only_workspace(&result).collection.items, "Create order");

    assert_eq!(create.body, "{\"sku\": \"{{$guid}}\"}");
    assert!(create.headers.iter().any(|h| h.key == "Content-Type" && h.value == "application/json"));
}

#[test]
fn form_bodies_are_converted_and_file_fields_are_reported() {
    const JSON: &str = r#"{ "__export_format": 4, "resources": [
      { "_id": "wrk_f", "_type": "workspace", "name": "Forms" },
      { "_id": "req_form", "_type": "request", "parentId": "wrk_f", "name": "Login", "method": "POST", "url": "https://x.example.test/login",
        "body": { "mimeType": "application/x-www-form-urlencoded", "params": [
          { "name": "user name", "value": "{{ _.user }}" },
          { "name": "off", "value": "1", "disabled": true } ] } },
      { "_id": "req_upload", "_type": "request", "parentId": "wrk_f", "name": "Upload", "method": "POST", "url": "https://x.example.test/upload",
        "body": { "mimeType": "multipart/form-data", "params": [
          { "name": "note", "value": "hello" },
          { "name": "avatar", "type": "file", "fileName": "/home/someone/avatar.png" } ] } },
      { "_id": "req_binary", "_type": "request", "parentId": "wrk_f", "name": "Binary", "method": "PUT", "url": "https://x.example.test/blob",
        "body": { "mimeType": "application/octet-stream", "fileName": "/home/someone/data.bin" } }
    ] }"#;

    let result = insomnia::import(JSON, "forms.json").unwrap();
    let items = &only_workspace(&result).collection.items;

    let login = find_request(items, "Login");
    assert_eq!(login.body, "user%20name={{user}}");
    assert!(login.headers.iter().any(|h| h.key == "Content-Type" && h.value == "application/x-www-form-urlencoded"));

    let upload = find_request(items, "Upload");
    assert!(upload.body.contains("name=\"note\""));
    assert!(!upload.body.contains("avatar"));
    assert!(result.issues.iter().any(|i| {
        i.category == ImportCategories::DROPPED_BODY && i.location == "Forms / Upload" && i.detail == "file form field \"avatar\""
    }));
    assert!(result.issues.iter().any(|i| {
        i.category == ImportCategories::DROPPED_BODY && i.location == "Forms / Binary" && i.detail == "file upload"
    }));
    assert_eq!(find_request(items, "Binary").body, "");
}

#[test]
fn unknown_placeholders_are_left_as_written_and_listed() {
    const JSON: &str = r#"{ "__export_format": 4, "resources": [
      { "_id": "wrk_d", "_type": "workspace", "name": "Dynamic" },
      { "_id": "req_d", "_type": "request", "parentId": "wrk_d", "name": "Session", "method": "GET",
        "url": "https://x.example.test/{% response 'body', 'req_1', 'b64::$.id::46b', 'never', 60 %}?n={{ uuid() }}" }
    ] }"#;

    let result = insomnia::import(JSON, "dynamic.json").unwrap();
    let request = find_request(&only_workspace(&result).collection.items, "Session");

    assert!(request.url.contains("{% response"));
    assert!(result.issues.iter().any(|i| i.category == ImportCategories::DYNAMIC_VARIABLES && i.location == "Dynamic / Session"));
}

#[test]
fn each_workspace_in_an_export_becomes_its_own_collection() {
    const JSON: &str = r#"{ "__export_format": 4, "resources": [
      { "_id": "wrk_a", "_type": "workspace", "name": "Alpha", "metaSortKey": 1 },
      { "_id": "wrk_b", "_type": "workspace", "name": "Beta", "metaSortKey": 2 },
      { "_id": "req_b", "_type": "request", "parentId": "wrk_b", "name": "Only in Beta", "method": "GET", "url": "https://b.example.test" }
    ] }"#;

    let result = insomnia::import(JSON, "two.json").unwrap();

    let names: Vec<&str> = result.workspaces.iter().map(|w| w.collection.name.as_str()).collect();
    assert_eq!(names, ["Alpha", "Beta"]);
    assert!(result.workspaces[0].collection.items.is_empty());
    let beta: Vec<&str> = result.workspaces[1].collection.items.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(beta, ["Only in Beta"]);
}

#[test]
fn descriptions_are_reported_as_not_imported() {
    let result = import_export();

    assert!(result.issues.iter().any(|i| i.category == ImportCategories::DROPPED_DATA && i.location == "1 description"));
}

#[test]
fn rejects_a_file_that_is_not_an_insomnia_export() {
    let not_export = insomnia::import(r#"{ "info": {}, "item": [] }"#, "x.json").unwrap_err();
    assert_eq!(not_export.message, "This is not an Insomnia export.");

    let not_json = insomnia::import("not json", "x.json").unwrap_err();
    assert!(not_json.message.starts_with("Not valid JSON: "), "message was {}", not_json.message);
}

#[test]
fn rejects_other_export_formats() {
    let error = insomnia::import(r#"{ "__export_format": 3, "resources": [] }"#, "x.json").unwrap_err();

    assert!(error.message.contains("format 3"), "message was {}", error.message);
}

#[test]
fn rejects_an_export_with_no_workspace() {
    let error = insomnia::import(r#"{ "__export_format": 4, "resources": [] }"#, "x.json").unwrap_err();

    assert_eq!(error.message, "This Insomnia export has no workspace to import.");
}

#[test]
fn unknown_auth_types_are_reported_and_not_imported() {
    const JSON: &str = r#"{ "__export_format": 4, "resources": [
      { "_id": "wrk_a", "_type": "workspace", "name": "Auth", "metaSortKey": 1 },
      { "_id": "fld_a", "_type": "request_group", "parentId": "wrk_a", "name": "Signed", "metaSortKey": 2,
        "authentication": { "type": "digest", "username": "tester" } },
      { "_id": "req_a", "_type": "request", "parentId": "fld_a", "name": "Call", "method": "GET", "url": "https://x.example.test/a",
        "metaSortKey": 3, "authentication": { "type": "inherit" } },
      { "_id": "req_b", "_type": "request", "parentId": "wrk_a", "name": "Other", "method": "GET", "url": "https://x.example.test/b",
        "metaSortKey": 4, "authentication": { "type": "ntlm" } }
    ] }"#;

    let result = insomnia::import(JSON, "auth.json").unwrap();
    let items = &only_workspace(&result).collection.items;

    assert!(find_request(items, "Call").auth.is_none());
    assert!(find_request(items, "Other").auth.is_none());
    assert!(result.issues.iter().any(|i| {
        i.category == ImportCategories::UNSUPPORTED_AUTH && i.location == "Auth / Signed" && i.detail == "digest"
    }));
    assert!(result.issues.iter().any(|i| {
        i.category == ImportCategories::UNSUPPORTED_AUTH && i.location == "Auth / Other" && i.detail == "ntlm"
    }));
    assert_eq!(result.issues.iter().filter(|i| i.category == ImportCategories::UNSUPPORTED_AUTH).count(), 2);
}

#[test]
fn requests_without_a_name_are_imported_as_untitled() {
    const JSON: &str = r#"{ "__export_format": 4, "resources": [
      { "_id": "wrk_n", "_type": "workspace", "name": "Nameless", "metaSortKey": 1 },
      { "_id": "req_n1", "_type": "request", "parentId": "wrk_n", "method": "GET", "url": "https://x.example.test/1", "metaSortKey": 2 },
      { "_id": "req_n2", "_type": "request", "parentId": "wrk_n", "name": "", "method": "DELETE", "url": "https://x.example.test/2", "metaSortKey": 3 }
    ] }"#;

    let result = insomnia::import(JSON, "nameless.json").unwrap();
    let items = &only_workspace(&result).collection.items;

    assert_eq!(items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["Untitled", "Untitled"]);
    assert_eq!(items.iter().map(|i| i.request.as_ref().unwrap().method.as_str()).collect::<Vec<_>>(), ["GET", "DELETE"]);
    assert!(result.summary.contains("2 requests"), "summary was {}", result.summary);
}

#[test]
fn a_workspace_without_a_name_takes_the_file_name() {
    const JSON: &str = r#"{ "__export_format": 4, "resources": [ { "_id": "wrk_x", "_type": "workspace" } ] }"#;

    let result = insomnia::import(JSON, "C:\\exports\\team-api.json").unwrap();

    assert_eq!(only_workspace(&result).collection.name, "team-api");
}

#[test]
fn report_text_groups_issues_by_category() {
    let text = import_export().to_report_text();

    assert!(text.starts_with("Imported workspace \"Shop\""), "text was {text}");
    assert!(text.contains(&format!("{} (", ImportCategories::DROPPED_DATA)), "text was {text}");
}
