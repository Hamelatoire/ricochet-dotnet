use std::time::Duration;

use pretty_assertions::assert_eq;
use ricochet_core::response_format::{FormattedBody, format};
use ricochet_core::send::ApiResponse;

fn response_bytes(body: &[u8], media_type: Option<&str>, charset: Option<&str>, truncated: bool) -> ApiResponse {
    ApiResponse {
        status_code: 200,
        reason_phrase: "OK".to_string(),
        headers: vec![],
        body: body.to_vec(),
        media_type: media_type.map(str::to_string),
        charset: charset.map(str::to_string),
        elapsed: Duration::ZERO,
        truncated,
        certificates_unverified: false,
    }
}

fn response(body: &str, media_type: Option<&str>) -> ApiResponse {
    response_bytes(body.as_bytes(), media_type, None, false)
}

fn format_response(body: &str, media_type: Option<&str>) -> FormattedBody {
    format(&response(body, media_type))
}

#[test]
fn pretty_prints_json_without_altering_numbers_or_unicode() {
    let result = format_response("{\"n\":12345678901234567890.10,\"s\":\"é\"}", Some("application/json"));

    assert!(result.is_json);
    assert!(!result.is_binary);
    assert!(result.text.contains("12345678901234567890.10"));
    assert!(result.text.contains('é'));
    assert!(result.text.contains('\n'));
}

#[test]
fn pretty_print_uses_two_space_indentation_and_keeps_key_order() {
    let result = format_response("{\"b\":[1,{}],\"a\":true}", Some("application/json"));

    assert_eq!(
        result.text,
        "{\n  \"b\": [\n    1,\n    {}\n  ],\n  \"a\": true\n}"
    );
}

#[test]
fn invalid_json_is_shown_as_is() {
    let result = format_response("{not json", Some("application/json"));

    assert!(!result.is_json);
    assert_eq!(result.text, "{not json");
}

#[test]
fn bom_does_not_break_json_detection() {
    let result = format(&response_bytes(&[0xEF, 0xBB, 0xBF, b'{', b'}'], Some("application/json"), None, false));

    assert!(result.is_json);
    assert_eq!(result.text, "{}");
}

#[test]
fn truncated_json_is_not_parsed() {
    let result = format(&response_bytes(b"{}", Some("application/json"), None, true));

    assert!(!result.is_json);
    assert_eq!(result.text, "{}");
}

#[test]
fn nul_bytes_mean_binary_unless_a_charset_says_otherwise() {
    let binary = format(&response_bytes(&[1, 0, 2], None, None, false));
    assert!(binary.is_binary);
    assert_eq!(binary.text, "[Binary response, 3 bytes]");

    let utf16 = format(&response_bytes(&[b'h', 0, b'i', 0], Some("text/plain"), Some("utf-16"), false));
    assert!(!utf16.is_binary);
    assert_eq!(utf16.text, "hi");
}

#[test]
fn binary_size_is_grouped_with_thousands_separators() {
    let result = format(&response_bytes(&vec![0u8; 1_234_567], None, None, false));

    assert_eq!(result.text, "[Binary response, 1,234,567 bytes]");
}

#[test]
fn unknown_charset_falls_back_to_utf8() {
    let result = format(&response_bytes(b"hi", Some("text/plain"), Some("not-a-charset"), false));

    assert_eq!(result.text, "hi");
}

#[test]
fn unknown_charset_still_counts_nul_bytes_as_binary() {
    let result = format(&response_bytes(&[1, 0, 2], Some("text/plain"), Some("not-a-charset"), false));

    assert!(result.is_binary);
}

#[test]
fn latin1_and_ascii_charsets_decode_their_bytes() {
    let latin1 = format(&response_bytes(&[0xE9], Some("text/plain"), Some("\"ISO-8859-1\""), false));
    assert_eq!(latin1.text, "é");

    let ascii = format(&response_bytes(&[b'a', 0xE9], Some("text/plain"), Some("us-ascii"), false));
    assert_eq!(ascii.text, "a\u{fffd}");
}

#[test]
fn utf16_big_endian_is_decoded() {
    let result = format(&response_bytes(&[0, b'h', 0, b'i'], Some("text/plain"), Some("utf-16BE"), false));

    assert_eq!(result.text, "hi");
}

#[test]
fn a_leading_byte_order_mark_is_removed_from_text() {
    let result = format(&response_bytes(&[0xEF, 0xBB, 0xBF, b'o', b'k'], Some("text/plain"), None, false));

    assert_eq!(result.text, "ok");
}

#[test]
fn a_non_json_media_type_is_pretty_printed_when_the_text_starts_like_json() {
    let result = format_response("  {\"a\":1}", Some("text/plain"));

    assert!(result.is_json);
    assert_eq!(result.text, "{\n  \"a\": 1\n}");
}

#[test]
fn text_that_is_not_json_is_shown_as_written() {
    let result = format_response("<p>hello</p>", Some("text/html"));

    assert!(!result.is_json);
    assert_eq!(result.text, "<p>hello</p>");
}

#[test]
fn nesting_up_to_the_depth_limit_is_pretty_printed_and_deeper_is_not() {
    let ok = format!("{}{}", "[".repeat(512), "]".repeat(512));
    assert!(format_response(&ok, Some("application/json")).is_json);

    let too_deep = format!("{}{}", "[".repeat(513), "]".repeat(513));
    assert!(!format_response(&too_deep, Some("application/json")).is_json);
}

#[test]
fn empty_json_body_is_not_an_error() {
    let result = format_response("", Some("application/json"));

    assert!(!result.is_json);
    assert_eq!(result.text, "");
}
