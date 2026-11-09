//! Tests for the Postman importer (`postman.rs`), using small fixtures written for these tests.

use std::collections::HashSet;
use std::io::{Cursor, Write};

use pretty_assertions::assert_eq;
use ricochet_core::models::{ApiRequest, auth_kinds};
use ricochet_core::postman::{self, EntryKind, ImportCategory, ImportResult};

const V21: &str = "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";

fn collection_json(items: &str, extra: &str) -> String {
    format!(r#"{{ "info": {{ "name": "Shop", "schema": "{V21}" }}, {extra} "item": [{items}] }}"#)
}

fn import(json: &str) -> ImportResult {
    postman::import(json, "file.json").expect("fixture should import")
}

fn only_request(result: &ImportResult) -> &ApiRequest {
    let items = &result.collection.as_ref().expect("a collection").items;
    assert_eq!(items.len(), 1);
    items[0].request.as_ref().expect("a request")
}

fn request_from(request_json: &str) -> ApiRequest {
    only_request(&import(&collection_json(
        &format!(r#"{{ "name": "R", "request": {request_json} }}"#,),
        "",
    )))
    .clone()
}

fn import_request(request_json: &str) -> ImportResult {
    import(&collection_json(
        &format!(r#"{{ "name": "R", "request": {request_json} }}"#),
        "",
    ))
}

fn issues_of(result: &ImportResult, category: ImportCategory) -> Vec<&postman::ImportIssue> {
    result
        .issues
        .iter()
        .filter(|i| i.category == category)
        .collect()
}

#[test]
fn keeps_folder_order_and_nesting() {
    let result = import(&collection_json(
        r#"
        { "name": "First", "request": { "method": "get", "url": "http://x.test/1" } },
        { "name": "Orders", "item": [
            { "name": "Nested", "item": [ { "name": "Deep", "request": { "method": "POST", "url": "http://x.test/2" } } ] }
        ] },
        { "name": "Last", "request": { "method": "GET", "url": "http://x.test/3" } }
        "#,
        "",
    ));

    let items = &result.collection.as_ref().unwrap().items;
    let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["First", "Orders", "Last"]);
    assert!(items[1].request.is_none());
    assert_eq!(
        items[1].children[0].children[0]
            .request
            .as_ref()
            .unwrap()
            .method,
        "POST"
    );
    assert_eq!(items[0].request.as_ref().unwrap().method, "GET");
    assert!(
        result.summary.contains("3 requests, 2 folders"),
        "{}",
        result.summary
    );
}

#[test]
fn reads_url_string_and_object_forms() {
    assert_eq!(request_from(r#"{ "url": "{{base}}/a" }"#).url, "{{base}}/a");
    assert_eq!(
        request_from(r#"{ "url": { "raw": "{{base}}/b?x=1" } }"#).url,
        "{{base}}/b?x=1"
    );
    let built = request_from(
        r#"{ "url": { "protocol": "https", "host": ["api", "example", "com"], "port": "8443", "path": ["v1", "items"],
            "query": [ { "key": "q", "value": "1" }, { "key": "off", "value": "2", "disabled": true } ] } }"#,
    );
    assert_eq!(built.url, "https://api.example.com:8443/v1/items?q=1");
}

#[test]
fn a_request_can_be_only_a_url_string() {
    let request = request_from(r#""http://x.test/plain""#);
    assert_eq!(request.method, "GET");
    assert_eq!(request.url, "http://x.test/plain");
}

#[test]
fn substitutes_path_variables_but_not_query_values() {
    let request = request_from(
        r#"{ "url": { "raw": "http://x.test/users/:id/posts/:postId?a=:id",
              "variable": [ { "key": "id", "value": "{{userId}}" }, { "key": "postId", "value": "9" } ] } }"#,
    );
    assert_eq!(request.url, "http://x.test/users/{{userId}}/posts/9?a=:id");
}

#[test]
fn disabled_query_parameter_is_reported() {
    let result = import_request(
        r#"{ "url": { "raw": "http://x.test", "query": [ { "key": "off", "value": "1", "disabled": true } ] } }"#,
    );
    let dropped = issues_of(&result, ImportCategory::DroppedData);
    assert_eq!(dropped.len(), 1);
    assert!(dropped[0].detail.contains("off"));
}

#[test]
fn reads_headers_including_disabled_and_the_string_form() {
    let list = request_from(
        r#"{ "url": "u", "header": [ { "key": "A", "value": "1" }, { "key": "B", "value": "2", "disabled": true } ] }"#,
    );
    assert!(list.headers[0].enabled);
    assert!(!list.headers[1].enabled);

    let text = request_from(r#"{ "url": "u", "header": "A: 1\nB: two: parts\n" }"#);
    assert_eq!(text.headers[1].value, "two: parts");
}

#[test]
fn raw_body_gets_its_content_type_unless_one_is_already_set() {
    let json = request_from(
        r#"{ "url": "u", "body": { "mode": "raw", "raw": "{}", "options": { "raw": { "language": "json" } } } }"#,
    );
    assert_eq!(json.body, "{}");
    assert_eq!(
        json.headers
            .iter()
            .find(|h| h.key == "Content-Type")
            .unwrap()
            .value,
        "application/json"
    );

    let explicit = request_from(
        r#"{ "url": "u", "header": [ { "key": "content-type", "value": "text/x" } ],
            "body": { "mode": "raw", "raw": "{}", "options": { "raw": { "language": "json" } } } }"#,
    );
    assert_eq!(explicit.headers.len(), 1);
    assert_eq!(explicit.headers[0].value, "text/x");
}

#[test]
fn urlencoded_body_escapes_values_but_keeps_placeholders() {
    let result = import_request(
        r#"{ "url": "u", "body": { "mode": "urlencoded", "urlencoded": [
            { "key": "a b", "value": "x&y" }, { "key": "t", "value": "pre {{token}}" }, { "key": "off", "value": "1", "disabled": true } ] } }"#,
    );

    let request = only_request(&result);
    assert_eq!(request.body, "a%20b=x%26y&t=pre%20{{token}}");
    assert_eq!(
        request.headers[0].value,
        "application/x-www-form-urlencoded"
    );
    assert!(
        issues_of(&result, ImportCategory::DroppedBody)
            .iter()
            .any(|i| i.detail.contains("off"))
    );
}

#[test]
fn formdata_text_fields_become_multipart_and_file_fields_are_reported() {
    let result = import_request(
        r#"{ "url": "u", "body": { "mode": "formdata", "formdata": [
            { "key": "title", "value": "hello", "type": "text" },
            { "key": "upload", "type": "file", "src": "/tmp/a.png" } ] } }"#,
    );

    let request = only_request(&result);
    let content_type = &request
        .headers
        .iter()
        .find(|h| h.key == "Content-Type")
        .unwrap()
        .value;
    assert_eq!(
        content_type,
        "multipart/form-data; boundary=----RicochetFormBoundary"
    );
    assert!(request.body.contains("name=\"title\""));
    assert!(!request.body.contains("upload"));
    assert!(request.body.ends_with("--\r\n"));
    assert!(
        issues_of(&result, ImportCategory::DroppedBody)
            .iter()
            .any(|i| i.detail.contains("upload"))
    );
}

#[test]
fn graphql_body_becomes_json_and_keeps_the_variables_in_their_order() {
    let request = request_from(
        r#"{ "url": "u", "body": { "mode": "graphql", "graphql": { "query": "{ me { id } }", "variables": "{\"b\":1,\"a\":2}" } } }"#,
    );

    assert_eq!(
        request.body,
        "{\n  \"query\": \"{ me { id } }\",\n  \"variables\": {\n    \"b\": 1,\n    \"a\": 2\n  }\n}"
    );
    assert_eq!(request.headers[0].value, "application/json");
}

#[test]
fn bodies_that_cannot_be_carried_over_are_reported_and_left_empty() {
    let bodies = [
        r#"{ "mode": "file", "file": { "src": "a.png" } }"#,
        r#"{ "mode": "somethingnew" }"#,
        r#"{ "mode": "raw", "raw": "x", "disabled": true }"#,
    ];
    for body in bodies {
        let result = import_request(&format!(r#"{{ "url": "u", "body": {body} }}"#));
        assert_eq!(only_request(&result).body, "", "body {body}");
        assert!(
            !issues_of(&result, ImportCategory::DroppedBody).is_empty(),
            "body {body}"
        );
    }
}

#[test]
fn reads_auth_in_both_the_v21_and_v20_shapes() {
    let v21 = request_from(
        r#"{ "url": "u", "auth": { "type": "bearer", "bearer": [ { "key": "token", "value": "{{t}}" } ] } }"#,
    );
    assert_eq!(v21.auth.unwrap().token, "{{t}}");

    let v20 = request_from(
        r#"{ "url": "u", "auth": { "type": "basic", "basic": { "username": "me", "password": "pw" } } }"#,
    );
    let basic = v20.auth.unwrap();
    assert_eq!(
        (basic.username.as_str(), basic.password.as_str()),
        ("me", "pw")
    );

    let api_key = request_from(
        r#"{ "url": "u", "auth": { "type": "apikey", "apikey": [ { "key": "key", "value": "k" }, { "key": "value", "value": "v" }, { "key": "in", "value": "query" } ] } }"#,
    );
    let key = api_key.auth.unwrap();
    assert_eq!(
        (
            key.key_name.as_str(),
            key.key_value.as_str(),
            key.key_in_query
        ),
        ("k", "v", true)
    );
    assert_eq!(key.kind, auth_kinds::API_KEY);
}

#[test]
fn auth_inherits_from_the_nearest_parent_and_noauth_stops_it() {
    let result = import(&collection_json(
        r#"
        { "name": "FromCollection", "request": { "url": "u" } },
        { "name": "Folder", "auth": { "type": "bearer", "bearer": [ { "key": "token", "value": "folder" } ] }, "item": [
            { "name": "FromFolder", "request": { "url": "u" } },
            { "name": "Explicit", "request": { "url": "u", "auth": { "type": "inherit" } } },
            { "name": "NoAuth", "request": { "url": "u", "auth": { "type": "noauth" } } }
        ] }
        "#,
        r#""auth": { "type": "bearer", "bearer": [ { "key": "token", "value": "root" } ] },"#,
    ));

    let items = &result.collection.as_ref().unwrap().items;
    assert_eq!(
        items[0]
            .request
            .as_ref()
            .unwrap()
            .auth
            .as_ref()
            .unwrap()
            .token,
        "root"
    );
    let children = &items[1].children;
    assert_eq!(
        children[0]
            .request
            .as_ref()
            .unwrap()
            .auth
            .as_ref()
            .unwrap()
            .token,
        "folder"
    );
    assert_eq!(
        children[1]
            .request
            .as_ref()
            .unwrap()
            .auth
            .as_ref()
            .unwrap()
            .token,
        "folder"
    );
    assert!(children[2].request.as_ref().unwrap().auth.is_none());
}

#[test]
fn unsupported_auth_is_reported_and_the_request_has_none() {
    let result = import_request(r#"{ "url": "u", "auth": { "type": "oauth2", "oauth2": [] } }"#);
    assert_eq!(result.issues.len(), 1);
    assert_eq!(result.issues[0].category, ImportCategory::UnsupportedAuth);
    assert_eq!(result.issues[0].detail, "oauth2");
    assert!(only_request(&result).auth.is_none());
}

#[test]
fn scripts_are_kept_and_reported_at_every_level_but_empty_stubs_are_ignored() {
    let real =
        r#""event": [ { "listen": "prerequest", "script": { "exec": ["pm.a();", "pm.b();"] } } ],"#;
    let stub = r#""event": [ { "listen": "test", "script": { "exec": [""] } } ],"#;
    let result = import(&collection_json(
        &format!(
            r#"{{ "name": "Folder", {real} "item": [
                {{ "name": "WithScript", {real} "request": {{ "url": "u" }} }},
                {{ "name": "WithStub", {stub} "request": {{ "url": "u" }} }}
            ] }}"#
        ),
        real,
    ));

    let collection = result.collection.as_ref().unwrap();
    assert_eq!(collection.scripts[0].source, "pm.a();\npm.b();");
    let folder = &collection.items[0];
    assert_eq!(folder.scripts.len(), 1);
    assert_eq!(folder.children[0].scripts.len(), 1);
    assert!(folder.children[1].scripts.is_empty());

    let reported: Vec<&str> = issues_of(&result, ImportCategory::Scripts)
        .iter()
        .map(|i| i.location.as_str())
        .collect();
    assert_eq!(reported.len(), 3);
    assert!(reported.contains(&"Shop / Folder / WithScript"));
    assert!(!reported.contains(&"Shop / Folder / WithStub"));
}

#[test]
fn request_test_scripts_are_kept_and_say_they_are_not_translated() {
    let result = import(&collection_json(
        r#"{ "name": "R", "event": [ { "listen": "test", "script": { "exec": ["pm.test('ok', () => {});"] } } ], "request": { "url": "u" } }"#,
        "",
    ));

    let request_item = &result.collection.as_ref().unwrap().items[0];
    assert_eq!(request_item.scripts.len(), 1);
    assert_eq!(request_item.scripts[0].listen, "test");
    let issue = issues_of(&result, ImportCategory::Scripts)
        .into_iter()
        .next()
        .unwrap();
    assert!(
        issue.detail.starts_with("test, not translated"),
        "{}",
        issue.detail
    );
    assert!(only_request(&result).assertions.is_none());
}

#[test]
fn dynamic_variables_stay_as_written_and_are_reported() {
    let result = import_request(
        r#"{ "url": "http://x.test/{{$randomColor}}", "body": { "mode": "raw", "raw": "{{$isbn}} {{normal}}" } }"#,
    );
    assert_eq!(only_request(&result).url, "http://x.test/{{$randomColor}}");
    let issues = issues_of(&result, ImportCategory::DynamicVariables);
    assert_eq!(issues.len(), 1);
    assert!(issues[0].detail.contains("$randomColor"));
    assert!(issues[0].detail.contains("$isbn"));
    assert!(!issues[0].detail.contains("normal"));
}

#[test]
fn dynamic_variables_with_a_built_in_function_are_not_reported() {
    // $guid, $timestamp and $randomInt resolve to built-in functions (ADR 0008), so nothing is left out.
    let result = import_request(
        r#"{ "url": "http://x.test/{{$guid}}", "body": { "mode": "raw", "raw": "{{$timestamp}} {{$randomInt}}" } }"#,
    );
    assert_eq!(only_request(&result).url, "http://x.test/{{$guid}}");
    assert!(issues_of(&result, ImportCategory::DynamicVariables).is_empty());
}

#[test]
fn examples_and_descriptions_are_counted_as_not_imported() {
    let result = import_request(r#"{ "url": "u", "description": "d" }"#);
    assert!(result.issues.iter().any(|i| i.location == "1 description"));

    let examples = import(&collection_json(
        r#"{ "name": "R", "request": { "url": "u" }, "response": [ { "name": "e1" }, { "name": "e2" } ] }"#,
        "",
    ));
    assert!(
        examples
            .issues
            .iter()
            .any(|i| i.location == "2 saved example responses")
    );
}

#[test]
fn reads_collection_variables_with_non_string_values() {
    let result = import(&collection_json(
        "",
        r#""variable": [ { "key": "a", "value": "1" }, { "key": "n", "value": 5 }, { "id": "byId", "value": true },
                        { "key": "off", "value": "x", "disabled": true }, { "key": "s", "value": "pw", "type": "secret" } ],"#,
    ));

    let variables = &result.collection.as_ref().unwrap().variables;
    assert_eq!(variables[1].value, "5");
    assert_eq!(
        (variables[2].key.as_str(), variables[2].value.as_str()),
        ("byId", "true")
    );
    assert!(!variables[3].enabled);
    assert!(variables[4].is_secret);
    assert!(
        issues_of(&result, ImportCategory::Secrets)
            .iter()
            .any(|i| i.detail == "s")
    );
}

#[test]
fn reads_environment_files_and_flags_secrets_without_values() {
    let result = import(
        r#"{ "name": "Staging", "_postman_variable_scope": "environment", "values": [
            { "key": "base", "value": "http://s.test", "enabled": true },
            { "key": "off", "value": "1", "enabled": false },
            { "key": "token", "value": "", "type": "secret", "enabled": true } ] }"#,
    );

    let environment = result.environment.as_ref().unwrap();
    assert_eq!(environment.name, "Staging");
    assert!(!environment.variables[1].enabled);
    assert!(result.globals.is_none());
    assert!(
        issues_of(&result, ImportCategory::Secrets)
            .iter()
            .any(|i| i.detail.contains("no value"))
    );
    assert_eq!(
        result.summary,
        "Imported environment \"Staging\" with 3 variables."
    );
}

#[test]
fn reads_globals_files() {
    let result = import(
        r#"{ "name": "g", "_postman_variable_scope": "globals", "values": [ { "key": "a", "value": "1" } ] }"#,
    );
    let globals = result.globals.as_ref().unwrap();
    assert_eq!(globals[0].key, "a");
    assert!(result.environment.is_none());
    assert_eq!(result.summary, "Imported 1 global variable.");
}

#[test]
fn unreadable_files_give_a_clear_error() {
    let cases = [
        ("{not json", "Not valid JSON"),
        ("[]", "not a Postman"),
        (r#"{ "hello": 1 }"#, "not a Postman"),
        (r#"{ "version": 1, "collections": [] }"#, "v1"),
        (
            r#"{ "info": { "schema": "https://schema.getpostman.com/json/collection/v1.0.0/collection.json" }, "item": [] }"#,
            "Unsupported",
        ),
    ];
    for (json, expected) in cases {
        let error = postman::import(json, "file.json").expect_err(json);
        assert!(
            error.message().contains(expected),
            "{json}: {}",
            error.message()
        );
    }
}

#[test]
fn malformed_structure_does_not_crash() {
    let cases = [
        r#"{ "info": 1, "item": 1 }"#,
        r#"{ "info": { "name": 5 }, "item": [ 1, "x", null, { "name": 7 } ] }"#,
        r#"{ "info": {}, "item": [ { "name": "R", "request": { "url": 5, "header": 5, "body": 5, "auth": 5, "method": null } } ] }"#,
    ];
    for json in cases {
        assert!(
            postman::import(json, "file.json")
                .unwrap()
                .collection
                .is_some(),
            "{json}"
        );
    }
}

#[test]
fn the_report_groups_issues_by_category() {
    let report = import_request(r#"{ "url": "u", "auth": { "type": "oauth2" } }"#).to_report_text();
    assert!(
        report.starts_with("Imported collection \"Shop\": 1 request, 0 folders, 0 variables.\n"),
        "{report}"
    );
    assert!(
        report.contains(&format!(
            "\n{} (1):\n",
            ImportCategory::UnsupportedAuth.label()
        )),
        "{report}"
    );
    assert!(report.contains("  Shop / R: oauth2\n"), "{report}");
}

#[test]
fn requests_in_nested_folders_inherit_auth_through_each_level_and_are_counted() {
    let result = import(&collection_json(
        r#"
        { "name": "Outer", "auth": { "type": "bearer", "bearer": [ { "key": "token", "value": "outer" } ] }, "item": [
            { "name": "Inner", "auth": { "type": "noauth" }, "item": [
                { "name": "Deep", "request": { "method": "GET", "url": "http://x.test/deep" } } ] },
            { "name": "Inner2", "item": [
                { "name": "Deep2", "request": { "method": "GET", "url": "http://x.test/deep2" } } ] }
        ] }
        "#,
        "",
    ));

    let outer = &result.collection.as_ref().unwrap().items[0];
    let deep = &outer.children[0].children[0];
    let deep2 = &outer.children[1].children[0];
    assert_eq!(deep.name, "Deep");
    assert!(deep.request.as_ref().unwrap().auth.is_none());
    let auth = deep2.request.as_ref().unwrap().auth.as_ref().unwrap();
    assert_eq!(
        (auth.kind.as_str(), auth.token.as_str()),
        (auth_kinds::BEARER, "outer")
    );
    assert!(
        result.summary.contains("2 requests, 3 folders"),
        "{}",
        result.summary
    );
}

#[test]
fn a_disabled_header_is_kept_disabled_and_is_not_reported() {
    let result = import_request(
        r#"{ "url": "u", "header": [ { "key": "X-Debug", "value": "1", "disabled": true }, { "key": "X-Live", "value": "2" } ] }"#,
    );

    let request = only_request(&result);
    assert!(
        request
            .headers
            .iter()
            .any(|h| h.key == "X-Debug" && h.value == "1" && !h.enabled)
    );
    assert!(
        request
            .headers
            .iter()
            .any(|h| h.key == "X-Live" && h.enabled)
    );
    assert!(result.issues.is_empty());
}

#[test]
fn file_fields_and_file_bodies_are_reported_with_their_location() {
    let result = import(&collection_json(
        r#"
        { "name": "Upload", "request": { "url": "u", "method": "POST", "body": { "mode": "formdata", "formdata": [
            { "key": "avatar", "type": "file", "src": "a.png" } ] } } },
        { "name": "Blob", "request": { "url": "u", "method": "PUT", "body": { "mode": "file", "file": { "src": "b.bin" } } } }
        "#,
        "",
    ));

    let items = &result.collection.as_ref().unwrap().items;
    assert_eq!(items[0].request.as_ref().unwrap().body, "");
    assert!(
        !items[0]
            .request
            .as_ref()
            .unwrap()
            .headers
            .iter()
            .any(|h| h.key == "Content-Type")
    );
    assert_eq!(items[1].request.as_ref().unwrap().body, "");
    let dropped = issues_of(&result, ImportCategory::DroppedBody);
    assert_eq!(dropped.len(), 2);
    assert!(
        dropped
            .iter()
            .any(|i| i.location == "Shop / Upload" && i.detail == "file form field \"avatar\"")
    );
    assert!(
        dropped
            .iter()
            .any(|i| i.location == "Shop / Blob" && i.detail == "file upload")
    );
}

#[test]
fn an_imported_request_keeps_placeholders_for_the_variable_layer() {
    let result = import(&collection_json(
        r#"{ "name": "R", "request": { "method": "POST", "url": "{{base}}/items",
            "auth": { "type": "bearer", "bearer": [ { "key": "token", "value": "{{token}}" } ] },
            "body": { "mode": "raw", "raw": "{\"who\":\"{{who}}\"}", "options": { "raw": { "language": "json" } } } } }"#,
        "",
    ));

    let request = only_request(&result);
    assert_eq!(request.url, "{{base}}/items");
    assert_eq!(request.auth.as_ref().unwrap().token, "{{token}}");
    assert_eq!(request.body, "{\"who\":\"{{who}}\"}");
}

// ---- Data dumps ----

const DUMP_COLLECTION_V21: &str = r#"{ "info": { "name": "Shop API", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" }, "item": [ { "name": "Ping", "request": { "url": "https://x.test/ping" } } ] }"#;

fn zip_of(entries: &[(&str, &str)]) -> Cursor<Vec<u8>> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, text) in entries {
        if name.ends_with('/') {
            writer.add_directory(*name, options).unwrap();
        } else {
            writer.start_file(*name, options).unwrap();
            writer.write_all(text.as_bytes()).unwrap();
        }
    }

    writer.finish().unwrap()
}

fn collection_named(name: &str) -> String {
    DUMP_COLLECTION_V21.replace("Shop API", name)
}

const DUMP_ENVIRONMENT: &str =
    r#"{ "name": "Staging", "values": [ { "key": "host", "value": "x.test", "enabled": true } ] }"#;
const DUMP_GLOBALS: &str = r#"{ "_postman_variable_scope": "globals", "values": [ { "key": "token", "value": "t", "enabled": true } ] }"#;

#[test]
fn a_dump_lists_every_collection_environment_and_globals_export() {
    let dump = zip_of(&[
        ("shop.json", DUMP_COLLECTION_V21),
        ("staging.json", DUMP_ENVIRONMENT),
        ("globals.json", DUMP_GLOBALS),
        ("billing.json", collection_named("Billing").as_str()),
    ]);

    let entries = postman::read_dump(dump).unwrap();

    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["Shop API", "Staging", "Globals", "Billing"]);
    assert_eq!(
        entries.iter().map(|e| e.kind).collect::<Vec<_>>(),
        [
            EntryKind::Collection,
            EntryKind::Environment,
            EntryKind::Globals,
            EntryKind::Collection
        ]
    );
    let files: Vec<&str> = entries.iter().map(|e| e.file_name.as_str()).collect();
    assert_eq!(
        files,
        ["shop.json", "staging.json", "globals.json", "billing.json"]
    );
}

#[test]
fn selecting_entries_returns_only_those_in_dump_order() {
    let dump = zip_of(&[
        ("shop.json", DUMP_COLLECTION_V21),
        ("staging.json", DUMP_ENVIRONMENT),
        ("billing.json", collection_named("Billing").as_str()),
    ]);
    let entries = postman::read_dump(dump).unwrap();

    // Selection is by file name, so the order the person ticked does not matter.
    let chosen_names: HashSet<String> = ["billing.json", "shop.json", "not-in-dump.json"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let chosen = postman::selected(&entries, &chosen_names);

    let names: Vec<&str> = chosen
        .iter()
        .map(|r| r.collection.as_ref().unwrap().name.as_str())
        .collect();
    assert_eq!(names, ["Shop API", "Billing"]);
}

#[test]
fn selecting_nothing_imports_nothing() {
    let entries = postman::read_dump(zip_of(&[("shop.json", DUMP_COLLECTION_V21)])).unwrap();
    assert!(postman::selected(&entries, &HashSet::new()).is_empty());
}

#[test]
fn entries_in_folders_are_listed_by_their_path_and_folder_entries_are_ignored() {
    let entries = postman::read_dump(zip_of(&[
        ("collections/", ""),
        ("collections/shop.json", DUMP_COLLECTION_V21),
    ]))
    .unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].file_name, "collections/shop.json");
    assert_eq!(entries[0].name, "Shop API");
}

#[test]
fn a_file_that_is_not_a_zip_is_refused() {
    let not_zip = Cursor::new(DUMP_COLLECTION_V21.as_bytes().to_vec());
    let error = postman::read_dump(not_zip).unwrap_err();
    assert!(error.message().contains("not a ZIP"), "{}", error.message());
}

#[test]
fn an_empty_zip_is_refused() {
    let error = postman::read_dump(zip_of(&[])).unwrap_err();
    assert!(error.message().contains("no files"), "{}", error.message());
}

#[test]
fn a_dump_with_a_file_that_is_not_json_is_refused_and_named() {
    let error = postman::read_dump(zip_of(&[
        ("shop.json", DUMP_COLLECTION_V21),
        ("notes.txt", "hello"),
    ]))
    .unwrap_err();
    assert!(error.message().contains("notes.txt"), "{}", error.message());
}

#[test]
fn a_dump_with_an_entry_that_is_not_a_postman_export_is_refused_and_named() {
    let error = postman::read_dump(zip_of(&[
        ("shop.json", DUMP_COLLECTION_V21),
        ("manifest.json", r#"{ "version": 3 }"#),
    ]))
    .unwrap_err();
    assert!(
        error.message().contains("manifest.json"),
        "{}",
        error.message()
    );
    assert!(
        error.message().contains("not a Postman collection"),
        "{}",
        error.message()
    );
}

#[test]
fn a_dump_with_invalid_json_names_the_entry() {
    let error = postman::read_dump(zip_of(&[("broken.json", "{ not json")])).unwrap_err();
    assert!(
        error.message().starts_with("broken.json:"),
        "{}",
        error.message()
    );
}

#[test]
fn a_dump_in_the_older_v1_format_is_refused() {
    let error = postman::read_dump(zip_of(&[(
        "old.json",
        r#"{ "version": 1, "collections": [] }"#,
    )]))
    .unwrap_err();
    assert!(error.message().contains("v1"), "{}", error.message());
}
