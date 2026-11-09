//! Ports the pure parts of `tests/Ricochet.Core.Tests/AssertionTests.cs`: checks and captures against a response.
//! Round trips through files, Expected values with Variables, and runs are tested with their ports.

use std::time::Duration;

use pretty_assertions::assert_eq;
use ricochet_core::assertions::*;
use ricochet_core::models::{ApiRequest, KeyValue};
use serde_json::{Value, json};

const JSON_BODY: &str = r#"{ "items": [ { "id": 1, "name": "Widget", "price": 19.50, "ok": true, "none": null } ], "a.b": "dotted", "count": 2, "length": "own" }"#;

fn response_with(body: &str, media_type: &str) -> ResponseSnapshot {
    ResponseSnapshot {
        status_code: 200,
        elapsed: Duration::from_millis(150),
        headers: vec![KeyValue::new("Content-Type", media_type), KeyValue::new("X-Trace", "abc")],
        body: body.to_string(),
    }
}

fn response() -> ResponseSnapshot {
    response_with(JSON_BODY, "application/json")
}

fn check_with(response: &ResponseSnapshot, source: &str, path: &str, op: &str, expected: &str) -> AssertionResult {
    let assertion = RequestAssertion {
        source: source.into(),
        path: path.into(),
        op: op.into(),
        expected: expected.into(),
        ..Default::default()
    };
    let mut results = evaluate(&[assertion], response);
    assert_eq!(results.len(), 1);
    results.remove(0)
}

fn check(source: &str, path: &str, op: &str, expected: &str) -> AssertionResult {
    check_with(&response(), source, path, op, expected)
}

fn capture(capture: RequestCapture, response: &ResponseSnapshot) -> CaptureOutcome {
    evaluate_captures(&[capture], response)
}

fn capture_of(source: &str, path: &str, variable: &str) -> RequestCapture {
    RequestCapture { source: source.into(), path: path.into(), variable: variable.into(), ..Default::default() }
}

#[test]
fn checks_status() {
    let cases = [
        (ops::EQUAL, "200", true),
        (ops::EQUAL, "404", false),
        (ops::NOT_EQUAL, "404", true),
        (ops::LESS_THAN, "300", true),
        (ops::GREATER_THAN, "300", false),
        (ops::MATCHES, r"^2\d\d$", true),
    ];
    for (op, expected, passed) in cases {
        assert_eq!(check(sources::STATUS, "", op, expected).passed, passed, "{op} {expected}");
    }
}

#[test]
fn checks_time() {
    assert!(check(sources::TIME, "", ops::LESS_THAN, "500").passed);
    assert!(!check(sources::TIME, "", ops::LESS_THAN, "100").passed);
}

#[test]
fn checks_headers_ignoring_name_case() {
    assert!(check(sources::HEADER, "content-type", ops::CONTAINS, "json").passed);
    assert!(check(sources::HEADER, "X-Missing", ops::NOT_EXISTS, "").passed);
    assert!(!check(sources::HEADER, "X-Trace", ops::NOT_EXISTS, "").passed);
    assert_eq!(check(sources::HEADER, "X-Missing", ops::EQUAL, "x").message, "not found");
}

#[test]
fn checks_body_text() {
    assert!(check(sources::BODY, "", ops::CONTAINS, "Widget").passed);
    assert!(check(sources::BODY, "", ops::NOT_CONTAINS, "Gadget").passed);
}

#[test]
fn finds_json_values() {
    let cases = [
        ("items[0].name", "Widget"),
        ("$.items[0].name", "Widget"),
        ("items[0]['name']", "Widget"),
        ("['a.b']", "dotted"),
        ("items[0].ok", "true"),
        ("items[0].none", "null"),
        ("items.length", "1"),
        ("length", "own"),
    ];
    for (path, expected) in cases {
        let result = check(sources::JSON, path, ops::EQUAL, expected);
        assert!(result.passed, "{path}: {}", result.message);
    }
}

#[test]
fn numbers_compare_by_value() {
    assert!(check(sources::JSON, "items[0].price", ops::EQUAL, "19.5").passed);
    assert!(check(sources::JSON, "count", ops::EQUAL, "2.0").passed);
}

#[test]
fn json_null_exists_but_a_missing_value_does_not() {
    assert!(check(sources::JSON, "items[0].none", ops::EXISTS, "").passed);
    assert!(!check(sources::JSON, "items[0].nope", ops::EXISTS, "").passed);
    assert!(check(sources::JSON, "items[5]", ops::NOT_EXISTS, "").passed);
}

#[test]
fn non_json_body_fails_with_a_clear_message() {
    let result = check_with(&response_with("<html>", "text/html"), sources::JSON, "a", ops::EXISTS, "");
    assert!(!result.passed);
    assert!(result.message.contains("not valid JSON"), "{}", result.message);
}

#[test]
fn malformed_path_and_pattern_fail_clearly_rather_than_panic() {
    assert!(check(sources::JSON, "items[0", ops::EXISTS, "").message.contains("invalid path"));
    assert!(check(sources::JSON, "items[x]", ops::EXISTS, "").message.contains("invalid path"));
    assert!(check(sources::STATUS, "", ops::MATCHES, "(").message.contains("invalid pattern"));
    assert!(check(sources::STATUS, "", ops::LESS_THAN, "soon").message.contains("not a number"));
    assert!(check(sources::JSON, "items[0].name", ops::LESS_THAN, "1").message.contains("not a number"));
    assert!(check("nonsense", "", ops::EQUAL, "1").message.contains("unknown"));
    assert!(check(sources::STATUS, "", "nonsense", "1").message.contains("unknown"));
}

#[test]
fn runaway_pattern_is_not_matched_and_does_not_hang() {
    let body = format!("\"{}!\"", "a".repeat(40));
    let result = check_with(&response_with(&body, "application/json"), sources::BODY, "", ops::MATCHES, "^(a+)+$");
    assert!(!result.passed);
}

#[test]
fn failure_shows_what_was_received_but_keeps_it_short() {
    let long_body = "x".repeat(1000);
    let result = check_with(&response_with(&long_body, "text/plain"), sources::BODY, "", ops::EQUAL, "y");
    assert!(result.message.starts_with("got xxx"));
    assert!(result.message.chars().count() < 300);
}

#[test]
fn disabled_assertions_are_skipped() {
    let assertion = RequestAssertion { enabled: false, expected: "999".into(), ..Default::default() };
    assert!(evaluate(&[assertion], &response()).is_empty());
}

#[test]
fn describes_each_check_in_words() {
    let status = RequestAssertion { expected: "200".into(), ..Default::default() };
    assert_eq!(describe(&status), "status equals 200");

    let exists = RequestAssertion { source: sources::JSON.into(), path: "a[0]".into(), op: ops::EXISTS.into(), ..Default::default() };
    assert_eq!(describe(&exists), "json a[0] exists");

    let unknown = RequestAssertion { op: "nonsense".into(), expected: "1".into(), ..Default::default() };
    assert_eq!(describe(&unknown), "status nonsense 1");
}

#[test]
fn assertion_and_capture_defaults_match_the_csharp_file() {
    let assertion: RequestAssertion = serde_json::from_str("{}").unwrap();
    assert_eq!(assertion, RequestAssertion::default());
    assert!(assertion.enabled);
    assert_eq!(assertion.source, sources::STATUS);
    assert_eq!(assertion.op, ops::EQUAL);

    let capture: RequestCapture = serde_json::from_str("{}").unwrap();
    assert!(capture.enabled);
    assert_eq!(capture.source, sources::JSON);
}

#[test]
fn assertions_round_trip_with_camel_case_names() {
    let assertion = RequestAssertion {
        enabled: false,
        source: sources::JSON.into(),
        path: "a[0]".into(),
        op: ops::EXISTS.into(),
        expected: String::new(),
    };
    let value = serde_json::to_value(&assertion).unwrap();
    assert_eq!(value, json!({ "enabled": false, "source": "json", "path": "a[0]", "op": "exists", "expected": "" }));
    let back: RequestAssertion = serde_json::from_value(value).unwrap();
    assert_eq!(back, assertion);
}

#[test]
fn an_assertion_in_a_request_file_reads_into_the_real_type() {
    let request: ApiRequest = serde_json::from_value(json!({
        "name": "R",
        "assertions": [{ "source": "status", "op": "equals", "expected": "200" }],
        "captures": [{ "source": "json", "path": "data.token", "variable": "token" }]
    }))
    .unwrap();
    assert_eq!(request.assertions.as_ref().unwrap()[0].expected, "200");
    assert_eq!(request.captures.as_ref().unwrap()[0].variable, "token");
}

#[test]
fn reads_json_header_and_body_for_captures() {
    let response = ResponseSnapshot {
        headers: vec![KeyValue::new("X-Id", "h1")],
        body: r#"{"data":{"token":"abc","count":3}}"#.into(),
        ..response()
    };

    assert_eq!(capture(capture_of(sources::JSON, "data.token", "t"), &response).values, vec![KeyValue::new("t", "abc")]);
    assert_eq!(capture(capture_of(sources::JSON, "data.count", "c"), &response).values, vec![KeyValue::new("c", "3")]);
    assert_eq!(capture(capture_of(sources::HEADER, "x-id", "h"), &response).values, vec![KeyValue::new("h", "h1")]);
    let body = capture(capture_of(sources::BODY, "", "b"), &response);
    assert!(body.values[0].value.contains("token"));
}

#[test]
fn finding_nothing_is_a_failure() {
    let cases = [
        (r#"{"a":1}"#, "b", "t", "not found"),
        (r#"{"b":null}"#, "b", "t", "not found"),
        ("<html>", "b", "t", "not valid JSON"),
        (r#"{"a":1}"#, "a[", "t", "invalid path"),
        (r#"{"a":1}"#, "a", " ", "no variable name"),
    ];
    for (body, path, variable, expected) in cases {
        let outcome = capture(capture_of(sources::JSON, path, variable), &response_with(body, "application/json"));
        assert!(outcome.values.is_empty(), "{body}");
        let failure = outcome.failures.first().unwrap_or_else(|| panic!("{body}: no failure"));
        assert!(failure.message.contains(expected), "{body}: {}", failure.message);
    }
}

#[test]
fn disabled_captures_are_skipped() {
    let disabled = RequestCapture { enabled: false, path: "nope".into(), variable: "t".into(), ..Default::default() };
    let outcome = capture(disabled, &response_with("{}", "application/json"));
    assert!(outcome.values.is_empty());
    assert!(outcome.failures.is_empty());
}

#[test]
fn captures_keep_values_and_failures_apart() {
    let captures = [capture_of(sources::JSON, "items[0].name", "name"), capture_of(sources::JSON, "nope", "missing")];
    let outcome = evaluate_captures(&captures, &response());
    assert_eq!(outcome.values, vec![KeyValue::new("name", "Widget")]);
    assert_eq!(outcome.failures.len(), 1);
    assert!(outcome.failures[0].description.starts_with("capture missing from json nope"));
}

#[test]
fn json_path_reports_only_malformed_paths_as_errors() {
    let root: Value = serde_json::from_str(JSON_BODY).unwrap();
    assert_eq!(JsonPath::find(&root, "nope").unwrap(), None);
    assert_eq!(JsonPath::find(&root, "items[9]").unwrap(), None);
    assert!(JsonPath::find(&root, "items[0").is_err());
    assert!(JsonPath::find(&root, "items[-1]").is_err());
    assert_eq!(JsonPath::find(&root, "items[0].name").unwrap(), Some(json!("Widget")));
}
