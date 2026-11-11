//! Ports `tests/Ricochet.Core.Tests/PostmanTestTranslatorTests.cs`: the translator on its own, and the importer's
//! use of it (translated checks attach to the Request, untranslated ones are kept and reported).

use std::time::Duration;

use pretty_assertions::assert_eq;
use ricochet_core::assertions::{RequestAssertion, ResponseSnapshot, evaluate, ops, sources};
use ricochet_core::models::KeyValue;
use ricochet_core::postman::{self, ImportCategory, ImportResult};
use ricochet_core::postman_test_translator::try_translate;

const V21: &str = "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";

fn translate(script: &str) -> Vec<RequestAssertion> {
    try_translate(script).unwrap_or_else(|reason| panic!("should translate, but: {reason}"))
}

fn not_translated(script: &str) -> String {
    match try_translate(script) {
        Ok(assertions) => panic!("should not translate, but got {assertions:?}"),
        Err(reason) => reason,
    }
}

fn single(script: &str) -> RequestAssertion {
    let mut assertions = translate(script);
    assert_eq!(assertions.len(), 1, "for {script}");
    assertions.remove(0)
}

fn response(status: i32, headers: Vec<KeyValue>, body: &str) -> ResponseSnapshot {
    ResponseSnapshot {
        status_code: status,
        elapsed: Duration::ZERO,
        headers,
        body: body.to_string(),
    }
}

fn collection_json(items: &str) -> String {
    format!(r#"{{ "info": {{ "name": "Shop", "schema": "{V21}" }}, "item": [{items}] }}"#)
}

fn import(json: &str) -> ImportResult {
    postman::import(json, "file.json").expect("fixture should import")
}

fn scripts_issues(result: &ImportResult) -> Vec<&postman::ImportIssue> {
    result
        .issues
        .iter()
        .filter(|i| i.category == ImportCategory::Scripts)
        .collect()
}

#[test]
fn each_recognised_check_becomes_one_assertion() {
    // (check, source, path, op, expected)
    let cases = [
        ("pm.response.to.have.status(200);", sources::STATUS, "", ops::EQUAL, "200"),
        ("pm.expect(pm.response.code).to.eql(201);", sources::STATUS, "", ops::EQUAL, "201"),
        ("pm.expect(pm.response.code).to.equal(204);", sources::STATUS, "", ops::EQUAL, "204"),
        ("pm.expect(pm.response.code).to.be.equal(404);", sources::STATUS, "", ops::EQUAL, "404"),
        ("pm.expect(pm.response.code === 200).to.be.true;", sources::STATUS, "", ops::EQUAL, "200"),
        ("pm.expect(pm.response.responseTime).to.be.below(300);", sources::TIME, "", ops::LESS_THAN, "300"),
        ("pm.expect(pm.response.responseTime).to.be.lessThan(250);", sources::TIME, "", ops::LESS_THAN, "250"),
        ("pm.response.to.have.header(\"X-Trace\");", sources::HEADER, "X-Trace", ops::EXISTS, ""),
        ("pm.response.to.have.header('Content-Type', 'application/json');", sources::HEADER, "Content-Type", ops::EQUAL, "application/json"),
        ("pm.expect(pm.response.json().name).to.eql(\"Widget\");", sources::JSON, "name", ops::EQUAL, "Widget"),
        ("pm.expect(pm.response.json().items[0].id).to.eql(1);", sources::JSON, "items[0].id", ops::EQUAL, "1"),
        ("pm.expect(pm.response.json().price).to.equal(19.5);", sources::JSON, "price", ops::EQUAL, "19.5"),
        ("pm.expect(pm.response.json().items.length).to.eql(3);", sources::JSON, "items.length", ops::EQUAL, "3"),
    ];

    for (check, source, path, op, expected) in cases {
        let assertion = single(check);
        assert_eq!(assertion.source, source, "source for {check}");
        assert_eq!(assertion.path, path, "path for {check}");
        assert_eq!(assertion.op, op, "op for {check}");
        assert_eq!(assertion.expected, expected, "expected for {check}");
        assert!(assertion.enabled, "enabled for {check}");
    }
}

#[test]
fn a_check_inside_pm_test_is_translated_for_function_and_arrow_forms() {
    let assertion = single(
        r#"pm.test("Status is 200", function () {
    pm.response.to.have.status(200);
});"#,
    );
    assert_eq!(assertion.expected, "200");

    let arrow = single(
        r#"pm.test('Fast', () => {
    pm.expect(pm.response.responseTime).to.be.below(500);
});"#,
    );
    assert_eq!(arrow.expected, "500");
}

#[test]
fn several_checks_in_one_script_become_several_assertions_in_order() {
    let assertions = translate(
        r#"pm.test("OK", function () {
    pm.response.to.have.status(200);
    pm.response.to.have.header("X-Trace");
});
// a comment is fine
const data = pm.response.json();
pm.expect(data.name).to.eql("Widget");"#,
    );

    let sources_in_order: Vec<&str> = assertions.iter().map(|a| a.source.as_str()).collect();
    assert_eq!(sources_in_order, [sources::STATUS, sources::HEADER, sources::JSON]);
    assert_eq!(assertions[2].path, "name");
}

#[test]
fn a_json_variable_declared_from_pm_response_json_is_followed() {
    let assertion = single(
        r#"const jsonData = pm.response.json();
pm.expect(jsonData.items[0].name).to.eql("Widget");"#,
    );
    assert_eq!(assertion.path, "items[0].name");
}

#[test]
fn anything_else_is_not_translated() {
    let scripts = [
        "pm.response.to.have.status(200);\npm.environment.set(\"token\", pm.response.json().token);",
        "if (pm.response.code === 200) { pm.response.to.have.status(200); }",
        "pm.response.to.have.status(200) && pm.response.to.have.header(\"X\");",
        "pm.expect(pm.response.json().id).to.exist;",
        "pm.expect(pm.response.text()).to.include(\"ok\");",
        "pm.expect(pm.response.code).to.be.oneOf([200, 201]);",
        "pm.response.to.have.status(\"OK\");",
        "pm.expect(pm.response.json().id).to.eql(1, \"id\");",
        "var total = 0;",
        "/* block comment */ pm.response.to.have.status(200);",
    ];

    for script in scripts {
        assert!(try_translate(script).is_err(), "should not translate: {script}");
    }
}

#[test]
fn a_partly_recognised_script_is_not_translated_at_all_and_names_the_line() {
    let reason = not_translated(
        r#"pm.test("Mixed", function () {
    pm.response.to.have.status(200);
    pm.environment.set("token", pm.response.json().token);
});"#,
    );
    assert!(reason.contains("pm.environment.set"), "{reason}");
}

#[test]
fn a_test_that_is_never_closed_is_not_translated() {
    let reason = not_translated(
        r#"pm.test("Open", function () {
    pm.response.to.have.status(200);"#,
    );
    assert!(reason.contains("closing"), "{reason}");
}

#[test]
fn a_json_check_on_another_variable_is_not_translated() {
    let reason = not_translated(
        r#"const other = pm.response.json();
const data = pm.response.json();
pm.expect(other.name).to.eql("x");"#,
    );
    assert_eq!(reason, r#"pm.expect(other.name).to.eql("x");"#);
}

#[test]
fn a_script_with_no_checks_is_not_translated() {
    assert!(not_translated("// only a comment\n").contains("no checks"));
    assert!(not_translated("pm.test(\"Empty\", function () {\n});").contains("no checks"));
}

#[test]
fn long_unrecognised_lines_are_shortened_in_the_reason() {
    let script = format!(
        "pm.expect(pm.response.json().a).to.eql(\"{}\") || other();",
        "x".repeat(300)
    );
    let reason = not_translated(&script);
    assert!(reason.ends_with('…'), "{reason}");
    assert!(reason.chars().count() < 150, "{reason}");
}

#[test]
fn the_translated_status_check_passes_and_fails_as_postman_would() {
    let assertion = single("pm.response.to.have.status(200);");
    let ok = response(200, vec![], "{}");
    let missing = response(404, vec![], "{}");

    assert!(evaluate(std::slice::from_ref(&assertion), &ok)[0].passed);
    assert!(!evaluate(std::slice::from_ref(&assertion), &missing)[0].passed);
}

#[test]
fn the_translated_json_check_reads_the_response_body() {
    let assertion = single(r#"pm.expect(pm.response.json().items[0].name).to.eql("Widget");"#);
    let body = r#"{ "items": [ { "name": "Widget" } ] }"#;

    assert!(evaluate(&[assertion], &response(200, vec![], body))[0].passed);
}

#[test]
fn importer_attaches_translated_checks_to_the_request_and_keeps_no_script() {
    let result = import(&collection_json(
        r#"{ "name": "Get", "request": { "url": "u" }, "event": [ { "listen": "test", "script": { "exec": [
            "pm.test(\"OK\", function () {",
            "    pm.response.to.have.status(200);",
            "});"
        ] } } ] }"#,
    ));

    let item = &result.collection.as_ref().unwrap().items[0];
    let request = item.request.as_ref().unwrap();
    let assertions = request.assertions.as_ref().unwrap();
    assert_eq!(assertions.len(), 1);
    assert_eq!(assertions[0].source, sources::STATUS);
    assert!(item.scripts.is_empty());
    assert!(scripts_issues(&result).is_empty());
}

#[test]
fn importer_reports_an_untranslated_request_test_with_the_first_line_it_could_not_translate() {
    let result = import(&collection_json(
        r#"{ "name": "Get", "request": { "url": "u" }, "event": [ { "listen": "test", "script": { "exec": [
            "pm.response.to.have.status(200);",
            "var ids = [];"
        ] } } ] }"#,
    ));

    let item = &result.collection.as_ref().unwrap().items[0];
    assert!(item.request.as_ref().unwrap().assertions.is_none());
    assert_eq!(item.scripts.len(), 1);

    let issues = scripts_issues(&result);
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].location, "Shop / Get");
    assert_eq!(issues[0].detail, "test, not translated: var ids = [];");
}

#[test]
fn importer_never_translates_prerequest_or_folder_scripts() {
    let result = import(&collection_json(
        r#"{ "name": "Folder", "event": [ { "listen": "test", "script": { "exec": ["pm.response.to.have.status(200);"] } } ], "item": [
            { "name": "Pre", "request": { "url": "u" }, "event": [ { "listen": "prerequest", "script": { "exec": ["pm.response.to.have.status(200);"] } } ] }
        ] }"#,
    ));

    let folder = &result.collection.as_ref().unwrap().items[0];
    assert_eq!(folder.scripts.len(), 1);

    let child = &folder.children[0];
    assert!(child.request.as_ref().unwrap().assertions.is_none());
    assert_eq!(child.scripts.len(), 1);
    assert_eq!(scripts_issues(&result).len(), 2);
}
