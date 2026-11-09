use pretty_assertions::assert_eq;
use ricochet_core::models::KeyValue;
use ricochet_core::response_compare::*;

fn response(body: &str, media_type: Option<&str>, headers: Vec<KeyValue>) -> ResponseInput {
    ResponseInput {
        status: 200,
        headers,
        body: body.as_bytes().to_vec(),
        media_type: media_type.map(str::to_string),
    }
}

fn json(body: &str) -> ResponseInput {
    response(body, Some("application/json"), vec![])
}

fn text(body: &str) -> ResponseInput {
    response(body, Some("text/plain"), vec![])
}

fn header(key: &str, value: &str) -> KeyValue {
    KeyValue::new(key, value)
}

#[test]
fn identical_bodies_are_all_same_rows() {
    let result = compare(&text("a\nb"), &text("a\nb"), &[]);

    assert!(result.body.iter().all(|row| row.kind == DiffKind::Same));
    assert_eq!(result.body.len(), 2);
    assert_eq!(result.body_note, None);
}

#[test]
fn a_changed_line_is_paired_on_both_sides() {
    let result = compare(&text("one\ntwo\nthree"), &text("one\n2\nthree"), &[]);

    let changed: Vec<_> = result.body.iter().filter(|row| row.kind != DiffKind::Same).collect();
    assert_eq!(changed.len(), 1);
    let row = changed[0];
    assert_eq!(row.kind, DiffKind::Changed);
    assert_eq!(row.previous.as_deref(), Some("two"));
    assert_eq!(row.previous_number, Some(2));
    assert_eq!(row.current.as_deref(), Some("2"));
    assert_eq!(row.current_number, Some(2));
}

#[test]
fn added_and_removed_lines_have_no_counterpart() {
    let result = compare(&text("a\nb"), &text("a\nc\nd"), &[]);

    let changed: Vec<_> = result.body.iter().filter(|row| row.kind == DiffKind::Changed).collect();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].previous.as_deref(), Some("b"));

    let added: Vec<_> = result.body.iter().filter(|row| row.kind == DiffKind::Added).collect();
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].current.as_deref(), Some("d"));
    assert_eq!(added[0].previous, None);
    assert_eq!(added[0].previous_number, None);
}

#[test]
fn a_removed_line_with_no_replacement_is_marked_removed() {
    let result = compare(&text("a\nb\nc"), &text("a\nc"), &[]);

    let removed: Vec<_> = result.body.iter().filter(|row| row.kind == DiffKind::Removed).collect();
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].previous.as_deref(), Some("b"));
    assert_eq!(removed[0].current, None);
    assert_eq!(removed[0].current_number, None);
}

#[test]
fn whitespace_inside_json_does_not_make_lines_differ() {
    // Both bodies pretty-print to the same text, so no row should differ.
    let result = compare(&json(r#"{"a":1,"b":2}"#), &json(r#"{ "a" : 1,   "b": 2 }"#), &[]);

    assert!(result.body.iter().all(|row| row.kind == DiffKind::Same));
}

#[test]
fn json_is_pretty_printed_with_key_order_and_number_text_kept() {
    let result = compare(&json(r#"{"z":1.50,"a":[1e3,true,null],"e":{}}"#), &json(r#"{"z":1.50,"a":[1e3,true,null],"e":{}}"#), &[]);

    let shown: Vec<&str> = result.body.iter().map(|row| row.previous.as_deref().unwrap()).collect();
    assert_eq!(
        shown,
        vec!["{", "  \"z\": 1.50,", "  \"a\": [", "    1e3,", "    true,", "    null", "  ],", "  \"e\": {}", "}"]
    );
}

#[test]
fn invalid_json_is_compared_as_written() {
    let result = compare(&json("{not json"), &json("{not json"), &[]);

    assert_eq!(result.body.len(), 1);
    assert_eq!(result.body[0].previous.as_deref(), Some("{not json"));
}

#[test]
fn binary_bodies_are_not_compared() {
    let binary = ResponseInput {
        status: 200,
        headers: vec![],
        body: vec![0, 1, 2, 3, 0, 255],
        media_type: Some("application/octet-stream".to_string()),
    };

    let result = compare(&binary, &text("text"), &[]);

    assert!(result.body.is_empty());
    assert!(result.body_note.is_some());
}

#[test]
fn headers_are_matched_by_name_ignoring_case_and_changes_are_marked() {
    let before = response("x", None, vec![header("X-Limit", "10"), header("Server", "a")]);
    let after = response("x", None, vec![header("x-limit", "20"), header("X-New", "yes")]);

    let result = compare(&before, &after, &[]);

    let limit = result.headers.iter().find(|h| h.name.eq_ignore_ascii_case("X-Limit")).unwrap();
    assert!(limit.changed());
    assert_eq!(limit.previous.as_deref(), Some("10"));
    assert_eq!(limit.current.as_deref(), Some("20"));

    let server = result.headers.iter().find(|h| h.name == "Server").unwrap();
    assert_eq!(server.current, None);
    assert!(server.changed());

    let added = result.headers.iter().find(|h| h.name == "X-New").unwrap();
    assert_eq!(added.previous, None);
    assert!(added.changed());
}

#[test]
fn repeated_headers_are_joined_before_comparing() {
    let before = response("x", None, vec![header("Set-Cookie", "a=1"), header("Set-Cookie", "b=2")]);
    let after = response("x", None, vec![header("Set-Cookie", "a=1, b=2")]);

    let result = compare(&before, &after, &[]);

    assert_eq!(result.headers.len(), 1);
    assert!(!result.headers[0].changed());
}

#[test]
fn secret_values_are_masked_in_body_and_headers() {
    const SECRET: &str = "sk-live-9876543210";
    let before = response(&format!("token={SECRET}"), Some("text/plain"), vec![header("X-Echo", SECRET)]);
    let after = response("token=other", Some("text/plain"), vec![header("X-Echo", "other")]);

    let result = compare(&before, &after, &[SECRET]);

    let body_text: Vec<&str> = result
        .body
        .iter()
        .flat_map(|row| [row.previous.as_deref(), row.current.as_deref()])
        .flatten()
        .collect();
    assert!(body_text.iter().all(|line| !line.contains(SECRET)));
    assert!(result.headers.iter().all(|h| h.previous.as_deref().map_or(true, |v| !v.contains(SECRET))));
    assert_eq!(result.headers[0].previous.as_deref(), Some(HIDDEN_VALUE));
}

#[test]
fn short_secrets_are_not_masked() {
    let result = compare(&text("abc"), &text("abd"), &["abc"]);

    assert_eq!(result.body[0].previous.as_deref(), Some("abc"));
}

#[test]
fn sensitive_headers_are_hidden_and_never_counted_as_changed() {
    let before = response("x", None, vec![header("Authorization", "Bearer one")]);
    let after = response("x", None, vec![header("Authorization", "Bearer two")]);

    let result = compare(&before, &after, &[]);

    assert_eq!(result.headers.len(), 1);
    let found = &result.headers[0];
    assert!(found.hidden);
    assert!(!found.changed());
    assert_eq!(found.previous.as_deref(), Some(HIDDEN_VALUE));
    assert_eq!(found.current.as_deref(), Some(HIDDEN_VALUE));
}
