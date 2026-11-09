//! Tests for `curl`, ported from `tests/Ricochet.Core.Tests/CurlGeneratorTests.cs` (ADR 0022).

use pretty_assertions::assert_eq;
use ricochet_core::curl::{MASK, generate, guess_content_type};
use ricochet_core::models::{ApiRequest, KeyValue, RequestAuth, auth_kinds};

fn request(method: &str, url: &str) -> ApiRequest {
    ApiRequest { method: method.into(), url: url.into(), ..Default::default() }
}

fn get() -> ApiRequest {
    request("GET", "https://api.example.com/pets")
}

fn auth(kind: &str) -> RequestAuth {
    RequestAuth { kind: kind.into(), ..Default::default() }
}

fn with_auth(mut req: ApiRequest, auth: RequestAuth) -> ApiRequest {
    req.auth = Some(auth);
    req
}

#[test]
fn a_get_request_with_no_extras_is_just_the_url() {
    assert_eq!(generate(&get(), &[], false), "curl 'https://api.example.com/pets'");
}

#[test]
fn other_methods_are_named_with_x_and_the_url_moves_to_the_next_line() {
    let text = generate(&request("post", "https://api.example.com/pets"), &[], false);

    assert_eq!(text, "curl -X POST \\\n  'https://api.example.com/pets'");
}

#[test]
fn a_get_in_any_case_is_still_a_plain_get() {
    assert_eq!(generate(&request("get", "https://api.example.com/pets"), &[], false), "curl 'https://api.example.com/pets'");
}

#[test]
fn the_url_is_trimmed() {
    assert_eq!(generate(&request("GET", "  https://api.example.com/pets  "), &[], false), "curl 'https://api.example.com/pets'");
}

#[test]
fn disabled_and_blank_headers_are_left_out() {
    let req = ApiRequest {
        headers: vec![
            KeyValue::new("Accept", "application/json"),
            KeyValue { enabled: false, ..KeyValue::new("X-Off", "1") },
            KeyValue::new("  ", "x"),
        ],
        ..get()
    };

    let text = generate(&req, &[], false);

    assert_eq!(text, "curl 'https://api.example.com/pets' \\\n  -H 'Accept: application/json'");
    assert!(!text.contains("X-Off"));
}

#[test]
fn header_names_are_trimmed_and_values_are_kept_as_they_are() {
    let req = ApiRequest { headers: vec![KeyValue::new("  X-Pad  ", " spaced ")], ..get() };

    assert_eq!(
        generate(&req, &[], false),
        "curl 'https://api.example.com/pets' \\\n  -H 'X-Pad:  spaced '"
    );
}

#[test]
fn a_body_gets_the_content_type_the_sender_would_use() {
    let req = ApiRequest { body: "{\"name\":\"Rex\"}".into(), ..request("POST", "https://api.example.com/pets") };

    assert_eq!(
        generate(&req, &[], false),
        "curl -X POST \\\n  'https://api.example.com/pets' \\\n  -H 'Content-Type: application/json' \\\n  --data-raw '{\"name\":\"Rex\"}'"
    );
}

#[test]
fn an_explicit_content_type_is_not_duplicated() {
    let req = ApiRequest {
        body: "name=Rex".into(),
        headers: vec![KeyValue::new("Content-Type", "application/x-www-form-urlencoded")],
        ..request("POST", "https://api.example.com/pets")
    };

    let text = generate(&req, &[], false);

    assert_eq!(text.matches("Content-Type").count(), 1);
    assert!(text.contains("-H 'Content-Type: application/x-www-form-urlencoded'"));
}

#[test]
fn an_explicit_content_type_is_matched_without_regard_to_case_and_surrounding_spaces() {
    let req = ApiRequest {
        body: "{}".into(),
        headers: vec![KeyValue::new(" content-type ", "text/x-custom")],
        ..request("POST", "https://api.example.com/pets")
    };

    let text = generate(&req, &[], false);

    assert!(!text.contains("application/json"));
    assert!(text.contains("-H 'content-type: text/x-custom'"));
}

#[test]
fn a_disabled_content_type_header_does_not_stop_the_guess() {
    let req = ApiRequest {
        body: "{}".into(),
        headers: vec![KeyValue { enabled: false, ..KeyValue::new("Content-Type", "text/plain") }],
        ..request("POST", "https://api.example.com/pets")
    };

    let text = generate(&req, &[], false);

    assert!(text.contains("-H 'Content-Type: application/json'"));
    assert!(!text.contains("text/plain"));
}

#[test]
fn a_get_with_a_body_still_sends_the_body() {
    let req = ApiRequest { body: "x".into(), ..get() };

    assert_eq!(
        generate(&req, &[], false),
        "curl 'https://api.example.com/pets' \\\n  -H 'Content-Type: text/plain; charset=utf-8' \\\n  --data-raw 'x'"
    );
}

#[test]
fn the_content_type_guess_looks_at_the_first_non_space_character() {
    assert_eq!(guess_content_type("{\"a\":1}"), "application/json");
    assert_eq!(guess_content_type("[1,2]"), "application/json");
    assert_eq!(guess_content_type("  \n\t{}"), "application/json");
    assert_eq!(guess_content_type("name=Rex"), "text/plain; charset=utf-8");
    assert_eq!(guess_content_type("<xml/>"), "text/plain; charset=utf-8");
    assert_eq!(guess_content_type(""), "text/plain; charset=utf-8");
}

#[test]
fn single_quotes_in_values_are_escaped_for_the_shell() {
    let req = request("GET", "https://api.example.com/it's");

    assert_eq!(generate(&req, &[], false), "curl 'https://api.example.com/it'\\''s'");
}

#[test]
fn single_quotes_in_a_body_are_escaped_for_the_shell() {
    let req = ApiRequest { body: "it's".into(), ..request("POST", "https://api.example.com/pets") };

    assert!(generate(&req, &[], false).ends_with("--data-raw 'it'\\''s'"));
}

#[test]
fn bearer_tokens_are_masked_unless_revealed() {
    let req = with_auth(get(), RequestAuth { token: "tok-123456".into(), ..auth(auth_kinds::BEARER) });

    let masked = generate(&req, &[], false);
    let revealed = generate(&req, &[], true);

    assert!(!masked.contains("tok-123456"));
    assert!(masked.contains("Authorization: Bearer ***"));
    assert!(revealed.contains("Authorization: Bearer tok-123456"));
}

#[test]
fn a_bearer_with_no_token_adds_no_header() {
    let req = with_auth(get(), auth(auth_kinds::BEARER));

    assert_eq!(generate(&req, &[], false), "curl 'https://api.example.com/pets'");
}

#[test]
fn basic_credentials_are_masked_unless_revealed() {
    let req = with_auth(
        get(),
        RequestAuth { username: "amy".into(), password: "pw-secret".into(), ..auth(auth_kinds::BASIC) },
    );

    assert!(generate(&req, &[], false).contains("-u 'amy:***'"));
    assert!(generate(&req, &[], true).contains("-u 'amy:pw-secret'"));
}

#[test]
fn basic_auth_is_sent_even_when_both_parts_are_empty() {
    let req = with_auth(get(), auth(auth_kinds::BASIC));

    assert_eq!(generate(&req, &[], false), "curl 'https://api.example.com/pets' \\\n  -u ':'");
}

#[test]
fn secret_values_given_by_the_caller_are_masked_everywhere() {
    let req = ApiRequest {
        headers: vec![KeyValue::new("X-Trace", "hidden-value")],
        body: "{\"k\":\"hidden-value\"}".into(),
        ..request("POST", "https://api.example.com/pets?key=hidden-value")
    };

    let text = generate(&req, &["hidden-value"], false);

    assert!(!text.contains("hidden-value"));
    assert!(text.contains("?key=***"));
    assert!(text.contains("-H 'X-Trace: ***'"));
    assert!(text.contains("--data-raw '{\"k\":\"***\"}'"));
}

#[test]
fn reveal_shows_caller_secrets_too() {
    let req = request("GET", "https://api.example.com/pets?key=hidden-value");

    assert!(generate(&req, &["hidden-value"], true).contains("?key=hidden-value"));
}

#[test]
fn an_empty_secret_masks_nothing() {
    let req = ApiRequest { body: "plain".into(), ..get() };

    assert_eq!(generate(&req, &[""], false), generate(&req, &[], false));
}

#[test]
fn a_longer_secret_is_masked_before_a_secret_inside_it() {
    let req = request("GET", "https://api.example.com/pets?x=abcdef");

    let text = generate(&req, &["abc", "abcdef"], false);

    assert!(text.contains("?x=***'"));
    assert!(!text.contains("def"));
}

#[test]
fn auth_values_are_masked_even_when_the_auth_kind_does_not_send_them() {
    let req = with_auth(
        request("GET", "https://api.example.com/pets?t=tok-123456"),
        RequestAuth { token: "tok-123456".into(), ..auth(auth_kinds::NONE) },
    );

    let text = generate(&req, &[], false);

    assert!(!text.contains("tok-123456"));
    assert!(text.contains("?t=***"));
    assert!(!text.contains("Authorization"));
}

#[test]
fn an_api_key_in_the_query_becomes_part_of_the_url() {
    let req = with_auth(
        get(),
        RequestAuth { key_name: "api_key".into(), key_value: "k e y".into(), key_in_query: true, ..auth(auth_kinds::API_KEY) },
    );

    let text = generate(&req, &[], true);

    assert_eq!(text, "curl 'https://api.example.com/pets?api_key=k%20e%20y'");
}

#[test]
fn an_api_key_in_the_query_is_added_after_an_existing_query_string() {
    let req = with_auth(
        request("GET", "https://api.example.com/pets?limit=2"),
        RequestAuth { key_name: "api_key".into(), key_value: "abc".into(), key_in_query: true, ..auth(auth_kinds::API_KEY) },
    );

    assert_eq!(generate(&req, &[], true), "curl 'https://api.example.com/pets?limit=2&api_key=abc'");
}

#[test]
fn an_api_key_in_the_query_escapes_reserved_characters_in_name_and_value() {
    let req = with_auth(
        get(),
        RequestAuth { key_name: "a&b".into(), key_value: "c=d/é".into(), key_in_query: true, ..auth(auth_kinds::API_KEY) },
    );

    assert_eq!(generate(&req, &[], true), "curl 'https://api.example.com/pets?a%26b=c%3Dd%2F%C3%A9'");
}

#[test]
fn an_api_key_in_the_query_is_masked_unless_revealed() {
    let req = with_auth(
        get(),
        RequestAuth { key_name: "api_key".into(), key_value: "abcdef123".into(), key_in_query: true, ..auth(auth_kinds::API_KEY) },
    );

    let masked = generate(&req, &[], false);

    assert_eq!(masked, "curl 'https://api.example.com/pets?api_key=***'");
    assert!(!masked.contains("-H"));
}

#[test]
fn an_api_key_in_a_header_is_masked_unless_revealed() {
    let req = with_auth(
        get(),
        RequestAuth { key_name: "X-Api-Key".into(), key_value: "abcdef123".into(), ..auth(auth_kinds::API_KEY) },
    );

    assert!(generate(&req, &[], false).contains("-H 'X-Api-Key: ***'"));
    assert!(generate(&req, &[], true).contains("-H 'X-Api-Key: abcdef123'"));
}

#[test]
fn an_api_key_with_no_name_adds_nothing() {
    let req = with_auth(get(), RequestAuth { key_value: "abc".into(), ..auth(auth_kinds::API_KEY) });

    assert_eq!(generate(&req, &[], true), "curl 'https://api.example.com/pets'");
}

#[test]
fn other_auth_kinds_add_nothing_to_the_command() {
    for kind in [auth_kinds::NONE, auth_kinds::DIGEST, auth_kinds::OAUTH1, auth_kinds::AWS_SIG_V4] {
        let req = with_auth(get(), RequestAuth { token: "t".into(), ..auth(kind) });

        assert_eq!(generate(&req, &[], true), "curl 'https://api.example.com/pets'", "kind {kind}");
    }
}

#[test]
fn the_mask_is_three_stars() {
    assert_eq!(MASK, "***");
}
