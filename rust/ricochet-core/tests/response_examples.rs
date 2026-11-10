use std::time::Duration;

use pretty_assertions::assert_eq;
use ricochet_core::models::{ApiRequest, KeyValue, RequestAuth, auth_kinds};
use ricochet_core::response_compare::HIDDEN_VALUE;
use ricochet_core::response_examples::{ExampleError, create, secrets_to_mask};
use ricochet_core::send::ApiResponse;

fn response(body: &str, headers: Vec<KeyValue>) -> ApiResponse {
    ApiResponse {
        status_code: 200,
        reason_phrase: "OK".to_string(),
        headers,
        body: body.as_bytes().to_vec(),
        media_type: Some("application/json".to_string()),
        charset: None,
        elapsed: Duration::from_millis(12),
        truncated: false,
        certificates_unverified: false,
    }
}

fn secret(key: &str, value: &str) -> KeyValue {
    KeyValue { is_secret: true, ..KeyValue::new(key, value) }
}

#[test]
fn keeps_status_name_and_body_of_a_response() {
    let example = create(
        "  Happy path ",
        &ApiRequest::default(),
        &[],
        &[],
        &response("{\"id\":1}", vec![KeyValue::new("Content-Type", "application/json")]),
    )
    .unwrap();

    assert_eq!(example.name, "Happy path");
    assert_eq!(example.status_code, 200);
    assert_eq!(example.reason_phrase, "OK");
    assert_eq!(example.media_type.as_deref(), Some("application/json"));
    assert!(example.body.contains("\"id\""));
    assert_eq!(example.headers, vec![KeyValue::new("Content-Type", "application/json")]);
}

#[test]
fn the_body_is_pretty_printed_like_the_response_panel() {
    let example = create("Pretty", &ApiRequest::default(), &[], &[], &response("{\"a\":1}", vec![])).unwrap();

    assert_eq!(example.body, "{\n  \"a\": 1\n}");
}

#[test]
fn hides_sensitive_response_headers_as_history_does() {
    let example = create(
        "Login",
        &ApiRequest::default(),
        &[],
        &[],
        &response(
            "{}",
            vec![
                KeyValue::new("Set-Cookie", "session=abc123"),
                KeyValue::new("Content-Type", "application/json"),
            ],
        ),
    )
    .unwrap();

    assert_eq!(example.headers[0].value, HIDDEN_VALUE);
    assert_eq!(example.headers[1].value, "application/json");
}

#[test]
fn a_sensitive_header_with_only_a_placeholder_is_not_hidden() {
    let example = create(
        "Placeholder",
        &ApiRequest::default(),
        &[],
        &[],
        &response("{}", vec![KeyValue::new("Authorization", "{{token}}")]),
    )
    .unwrap();

    assert_eq!(example.headers[0].value, "{{token}}");
}

#[test]
fn masks_the_credentials_the_request_sent_wherever_they_are_echoed() {
    let sent = ApiRequest {
        url: "http://x.test/me".to_string(),
        headers: vec![KeyValue::new("Authorization", "Bearer sk-live-9999")],
        auth: Some(RequestAuth {
            kind: auth_kinds::BEARER.to_string(),
            token: "tok-secret-1234".to_string(),
            ..RequestAuth::default()
        }),
        ..ApiRequest::default()
    };

    let example = create(
        "Me",
        &sent,
        &[],
        &[],
        &response(
            "{\"token\":\"tok-secret-1234\",\"auth\":\"Bearer sk-live-9999\"}",
            vec![KeyValue::new("X-Echo", "tok-secret-1234")],
        ),
    )
    .unwrap();

    assert!(!example.body.contains("tok-secret-1234"));
    assert!(!example.body.contains("sk-live-9999"));
    assert!(!example.headers[0].value.contains("tok-secret-1234"));
    assert!(example.body.contains(HIDDEN_VALUE));
}

#[test]
fn masks_secret_variables_and_captured_values() {
    let variables = [secret("pw", "hunter22"), KeyValue::new("plain", "visible-text")];
    let captured = [KeyValue::new("session", "cap-value-77")];

    let example = create(
        "Echo",
        &ApiRequest::default(),
        &variables,
        &captured,
        &ApiResponse {
            media_type: None,
            ..response("hunter22 visible-text cap-value-77", vec![])
        },
    )
    .unwrap();

    assert_eq!(example.body, format!("{HIDDEN_VALUE} visible-text {HIDDEN_VALUE}"));
}

#[test]
fn values_shorter_than_four_characters_are_not_masked() {
    let variables = [secret("pin", "123")];

    let example = create(
        "Short",
        &ApiRequest::default(),
        &variables,
        &[],
        &ApiResponse {
            media_type: None,
            ..response("1234 and 123", vec![])
        },
    )
    .unwrap();

    assert_eq!(example.body, "1234 and 123");
}

#[test]
fn longer_secrets_are_masked_before_the_shorter_ones_they_contain() {
    let variables = [secret("short", "abcd"), secret("long", "abcdefgh")];

    let example = create(
        "Nested",
        &ApiRequest::default(),
        &variables,
        &[],
        &ApiResponse {
            media_type: None,
            ..response("abcdefgh", vec![])
        },
    )
    .unwrap();

    assert_eq!(example.body, HIDDEN_VALUE);
}

#[test]
fn keeps_placeholders_that_were_not_resolved() {
    let sent = ApiRequest {
        headers: vec![KeyValue::new("Authorization", "{{token}}")],
        ..ApiRequest::default()
    };

    let example = create("Unresolved", &sent, &[], &[], &response("{{token}}", vec![])).unwrap();

    assert_eq!(example.body, "{{token}}");
}

#[test]
fn a_disabled_sensitive_header_is_not_treated_as_a_secret() {
    let sent = ApiRequest {
        headers: vec![KeyValue { enabled: false, ..KeyValue::new("Authorization", "Bearer off-9999") }],
        ..ApiRequest::default()
    };

    let example = create("Disabled", &sent, &[], &[], &response("Bearer off-9999", vec![])).unwrap();

    assert_eq!(example.body, "Bearer off-9999");
}

#[test]
fn secrets_to_mask_are_longest_first_and_skip_short_and_plain_values() {
    let variables = [secret("a", "medium-value"), secret("b", "x"), KeyValue::new("c", "not-secret-value")];
    let captured = [KeyValue::new("d", "medium-value"), KeyValue::new("e", "tiny")];

    let masks = secrets_to_mask(&variables, &[], &captured, &ApiRequest::default());

    assert_eq!(masks, vec!["medium-value".to_string(), "tiny".to_string()]);
}

#[test]
fn refuses_a_truncated_response() {
    let truncated = ApiResponse { truncated: true, ..response("partial", vec![]) };

    let error = create("Big", &ApiRequest::default(), &[], &[], &truncated).unwrap_err();

    assert_eq!(error, ExampleError::TooLarge);
    assert!(error.message().contains("too large"));
}

#[test]
fn refuses_a_binary_response() {
    let binary = ApiResponse {
        body: vec![0, 1, 2, 0, 255, 0, 3],
        media_type: Some("application/octet-stream".to_string()),
        ..response("", vec![])
    };

    assert_eq!(create("Binary", &ApiRequest::default(), &[], &[], &binary).unwrap_err(), ExampleError::Binary);
}
