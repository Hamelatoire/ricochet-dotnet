use pretty_assertions::assert_eq;
use ricochet_core::collection_export::*;
use ricochet_core::models::*;

const SECRET: &str = "s3cr3t-value-123";

fn secret_var(key: &str, value: &str) -> KeyValue {
    KeyValue { key: key.into(), value: value.into(), is_secret: true, ..KeyValue::default() }
}

fn collection(variables: Vec<KeyValue>) -> Collection {
    Collection { name: "Shop".into(), variables, ..Collection::default() }
}

fn request(url: &str) -> ApiRequest {
    ApiRequest { url: url.into(), ..ApiRequest::default() }
}

fn header(key: &str, value: &str) -> KeyValue {
    KeyValue::new(key, value)
}

fn prepare(collection: &Collection, request: &ApiRequest) -> (PreparedRequest, ExportContext) {
    let mut context = ExportContext::new(collection);
    let prepared = PreparedRequest::create(&mut context, request, "Shop / Get order");
    (prepared, context)
}

// --- Report text -----------------------------------------------------------------------------------------

#[test]
fn report_text_is_empty_without_notes() {
    let result = ExportResult { json: "{}".into(), notes: vec![] };
    assert_eq!(result.to_report_text(), "");
}

#[test]
fn report_text_groups_by_category_in_first_seen_order_with_counts() {
    let result = ExportResult {
        json: String::new(),
        notes: vec![
            ExportNote::new(categories::NOT_EXPORTED, "Shop", "2 assertions"),
            ExportNote::new(categories::SECRET_VARIABLES, "Shop", "apiKey"),
            ExportNote::new(categories::NOT_EXPORTED, "Shop / Get", "1 capture"),
        ],
    };

    assert_eq!(
        result.to_report_text(),
        "Not exported (2):\n  Shop: 2 assertions\n  Shop / Get: 1 capture\n\nSecret Variables left out (1):\n  Shop: apiKey"
    );
}

#[test]
fn report_text_shows_only_the_location_when_there_is_no_detail() {
    let result = ExportResult {
        json: String::new(),
        notes: vec![ExportNote::new(categories::NOT_EXPORTED, "Shop / Get", "")],
    };

    assert_eq!(result.to_report_text(), "Not exported (1):\n  Shop / Get");
}

#[test]
fn plural_and_placeholder_formats() {
    assert_eq!(plural(1, "folder"), "1 folder");
    assert_eq!(plural(0, "folder"), "0 folders");
    assert_eq!(plural(3, "assertion"), "3 assertions");
    assert_eq!(placeholder("apiKey"), "{{apiKey}}");
}

// --- Secret Variables ------------------------------------------------------------------------------------

#[test]
fn secret_variable_without_a_value_is_noted_as_left_out() {
    let context = ExportContext::new(&collection(vec![secret_var("token", ""), var_plain("host", "example.test")]));

    assert_eq!(
        context.notes,
        vec![ExportNote::new(
            categories::SECRET_VARIABLES,
            "Shop",
            "token (has no value on this machine)",
        )]
    );
}

#[test]
fn secret_variable_with_a_value_is_noted_by_name_only() {
    let context = ExportContext::new(&collection(vec![secret_var("apiKey", SECRET)]));

    assert_eq!(context.notes, vec![ExportNote::new(categories::SECRET_VARIABLES, "Shop", "apiKey")]);
    assert!(!format!("{:?}", context.notes).contains(SECRET));
}

fn var_plain(key: &str, value: &str) -> KeyValue {
    KeyValue::new(key, value)
}

// --- Secret values never reach an export -----------------------------------------------------------------

#[test]
fn secret_value_in_url_and_body_is_replaced_with_its_placeholder() {
    let col = collection(vec![secret_var("apiKey", SECRET)]);
    let req = ApiRequest {
        url: format!("https://api.test/orders?key={SECRET}"),
        body: format!("{{\"key\": \"{SECRET}\"}}"),
        ..ApiRequest::default()
    };

    let (prepared, context) = prepare(&col, &req);

    assert_eq!(prepared.url, "https://api.test/orders?key={{apiKey}}");
    assert_eq!(prepared.body, "{\"key\": \"{{apiKey}}\"}");
    assert_eq!(
        context.notes,
        vec![
            ExportNote::new(categories::SECRET_VARIABLES, "Shop", "apiKey"),
            ExportNote::new(
                categories::SECRET_VALUES,
                "Shop / Get order",
                "the value of secret apiKey was replaced with {{apiKey}}",
            ),
            ExportNote::new(
                categories::SECRET_VALUES,
                "Shop / Get order body",
                "the value of secret apiKey was replaced with {{apiKey}}",
            ),
        ]
    );
}

#[test]
fn secret_value_never_appears_anywhere_in_the_prepared_request_or_report() {
    let col = collection(vec![secret_var("apiKey", SECRET)]);
    let req = ApiRequest {
        method: "post".into(),
        url: format!("https://api.test/{SECRET}/x"),
        headers: vec![
            header("X-Key", SECRET),
            header("Accept", &format!("application/json; v={SECRET}")),
        ],
        body: format!("token={SECRET}"),
        ..ApiRequest::default()
    };

    let (prepared, context) = prepare(&col, &req);
    let report = ExportResult { json: String::new(), notes: context.notes.clone() }.to_report_text();

    assert!(!format!("{prepared:?}").contains(SECRET), "prepared request holds the Secret: {prepared:?}");
    assert!(!report.contains(SECRET), "report holds the Secret: {report}");
    assert_eq!(prepared.url, "https://api.test/{{apiKey}}/x");
    assert_eq!(prepared.headers[0].value, "{{apiKey}}");
    assert_eq!(prepared.headers[1].value, "application/json; v={{apiKey}}");
    assert_eq!(prepared.body, "token={{apiKey}}");
}

#[test]
fn longer_secret_value_is_replaced_whole_before_a_shorter_one_inside_it() {
    let col = collection(vec![secret_var("short", "abc"), secret_var("long", "abcdef")]);
    let req = request("https://api.test/abcdef");

    let (prepared, _context) = prepare(&col, &req);

    assert_eq!(prepared.url, "https://api.test/{{long}}");
}

#[test]
fn secret_variable_without_value_leaves_the_placeholder_in_place() {
    // A reference like {{token}} is not a value, so it is written as it is.
    let col = collection(vec![secret_var("token", "")]);
    let req = request("https://api.test/?t={{token}}");

    let (prepared, _context) = prepare(&col, &req);

    assert_eq!(prepared.url, "https://api.test/?t={{token}}");
}

// --- Typed Secrets (ADR 0004) ----------------------------------------------------------------------------

#[test]
fn bearer_token_typed_into_a_request_is_written_as_a_placeholder() {
    let col = collection(vec![]);
    let req = ApiRequest {
        auth: Some(RequestAuth {
            kind: auth_kinds::BEARER.into(),
            token: "eyJhbGciOi.real.token".into(),
            ..RequestAuth::default()
        }),
        ..ApiRequest::default()
    };

    let (prepared, _context) = prepare(&col, &req);

    assert_eq!(prepared.auth.kind, auth_kinds::BEARER);
    assert_eq!(prepared.auth.token, "{{token}}");
    assert_eq!(
        prepared.typed_secrets,
        vec![TypedSecret { field: "bearer token".into(), placeholder: "{{token}}".into() }]
    );
    assert!(!format!("{prepared:?}").contains("eyJhbGciOi"));
}

#[test]
fn bearer_token_that_is_already_a_placeholder_is_kept() {
    let col = collection(vec![]);
    let req = ApiRequest {
        auth: Some(RequestAuth {
            kind: auth_kinds::BEARER.into(),
            token: "{{accessToken}}".into(),
            ..RequestAuth::default()
        }),
        ..ApiRequest::default()
    };

    let (prepared, _context) = prepare(&col, &req);

    assert_eq!(prepared.auth.token, "{{accessToken}}");
    assert!(prepared.typed_secrets.is_empty());
}

#[test]
fn sensitive_header_with_a_literal_value_is_typed_and_replaced() {
    let col = collection(vec![]);
    let req = ApiRequest {
        headers: vec![header("  Authorization ", "Bearer real-token")],
        ..ApiRequest::default()
    };

    let (prepared, _context) = prepare(&col, &req);

    assert_eq!(prepared.headers, vec![KeyValue::new("Authorization", "{{Authorization}}")]);
    assert_eq!(
        prepared.typed_secrets,
        vec![TypedSecret { field: "header Authorization".into(), placeholder: "{{Authorization}}".into() }]
    );
    assert!(!format!("{prepared:?}").contains("real-token"));
}

#[test]
fn sensitive_header_with_only_a_placeholder_is_kept() {
    let col = collection(vec![]);
    let req = ApiRequest {
        headers: vec![header("Cookie", "{{sessionId}}")],
        ..ApiRequest::default()
    };

    let (prepared, _context) = prepare(&col, &req);

    assert_eq!(prepared.headers[0].value, "{{sessionId}}");
    assert!(prepared.typed_secrets.is_empty());
}

#[test]
fn ordinary_header_with_a_literal_value_is_kept() {
    let col = collection(vec![]);
    let req = ApiRequest {
        headers: vec![header("X-Trace", "abc123")],
        ..ApiRequest::default()
    };

    let (prepared, _context) = prepare(&col, &req);

    assert_eq!(prepared.headers[0].value, "abc123");
    assert!(prepared.typed_secrets.is_empty());
}

#[test]
fn basic_auth_password_is_typed_and_username_is_kept() {
    let col = collection(vec![]);
    let req = ApiRequest {
        auth: Some(RequestAuth {
            kind: auth_kinds::BASIC.into(),
            username: "alice".into(),
            password: "hunter2-literal".into(),
            ..RequestAuth::default()
        }),
        ..ApiRequest::default()
    };

    let (prepared, _context) = prepare(&col, &req);

    assert_eq!(prepared.auth.username, "alice");
    assert_eq!(prepared.auth.password, "{{password}}");
    assert_eq!(
        prepared.typed_secrets,
        vec![TypedSecret { field: "basic password".into(), placeholder: "{{password}}".into() }]
    );
    assert!(!format!("{prepared:?}").contains("hunter2-literal"));
}

#[test]
fn api_key_value_is_typed_under_its_key_name_and_keeps_query_placement() {
    let col = collection(vec![]);
    let req = ApiRequest {
        auth: Some(RequestAuth {
            kind: auth_kinds::API_KEY.into(),
            key_name: "X-Api-Token".into(),
            key_value: "literal-key-value".into(),
            key_in_query: true,
            ..RequestAuth::default()
        }),
        ..ApiRequest::default()
    };

    let (prepared, _context) = prepare(&col, &req);

    assert_eq!(prepared.auth.key_name, "X-Api-Token");
    assert_eq!(prepared.auth.key_value, "{{X-Api-Token}}");
    assert!(prepared.auth.key_in_query);
    assert_eq!(
        prepared.typed_secrets,
        vec![TypedSecret { field: "API key X-Api-Token".into(), placeholder: "{{X-Api-Token}}".into() }]
    );
    assert!(!format!("{prepared:?}").contains("literal-key-value"));
}

#[test]
fn api_key_name_that_cannot_be_a_placeholder_falls_back_to_apikey() {
    let col = collection(vec![]);
    let req = ApiRequest {
        auth: Some(RequestAuth {
            kind: auth_kinds::API_KEY.into(),
            key_name: "my key".into(),
            key_value: "literal-key-value".into(),
            ..RequestAuth::default()
        }),
        ..ApiRequest::default()
    };

    let (prepared, _context) = prepare(&col, &req);

    assert_eq!(prepared.auth.key_value, "{{apiKey}}");
}

#[test]
fn auth_kinds_that_are_not_written_are_dropped_with_their_secrets() {
    let col = collection(vec![]);
    let req = ApiRequest {
        auth: Some(RequestAuth {
            kind: auth_kinds::DIGEST.into(),
            username: "alice".into(),
            password: "digest-password-literal".into(),
            ..RequestAuth::default()
        }),
        ..ApiRequest::default()
    };

    let (prepared, _context) = prepare(&col, &req);

    assert_eq!(prepared.auth, RequestAuth::default());
    assert!(prepared.typed_secrets.is_empty());
    assert!(!format!("{prepared:?}").contains("digest-password-literal"));
}

#[test]
fn request_without_auth_gets_no_auth() {
    let col = collection(vec![]);
    let (prepared, _context) = prepare(&col, &request("https://api.test/"));

    assert_eq!(prepared.auth, RequestAuth::default());
}

// --- Other fields of a prepared request ------------------------------------------------------------------

#[test]
fn method_is_uppercased_and_blank_header_names_are_dropped() {
    let col = collection(vec![]);
    let req = ApiRequest {
        method: "post".into(),
        headers: vec![
            header("   ", "ignored"),
            KeyValue { key: "X-Off".into(), value: "1".into(), enabled: false, ..KeyValue::default() },
        ],
        ..ApiRequest::default()
    };

    let (prepared, _context) = prepare(&col, &req);

    assert_eq!(prepared.method, "POST");
    assert_eq!(prepared.headers.len(), 1);
    assert_eq!(prepared.headers[0].key, "X-Off");
    assert!(!prepared.headers[0].enabled);
}

#[test]
fn content_type_ignores_case_and_disabled_headers() {
    let headers = vec![
        KeyValue { key: "content-type".into(), value: "text/plain".into(), enabled: false, ..KeyValue::default() },
        header("CONTENT-TYPE", "application/json"),
    ];

    assert_eq!(content_type(&headers), Some("application/json"));
    assert_eq!(content_type(&[]), None);
}

#[test]
fn clean_keeps_safe_names_and_falls_back_otherwise() {
    assert_eq!(clean("api_key-1.v2", "fallback"), "api_key-1.v2");
    assert_eq!(clean("", "fallback"), "fallback");
    assert_eq!(clean("has space", "fallback"), "fallback");
    assert_eq!(clean("café", "fallback"), "fallback");
}

#[test]
fn is_literal_and_sensitive_header_rules() {
    assert!(is_literal("abc"));
    assert!(is_literal("Bearer {{token}} extra"));
    assert!(!is_literal("{{token}}"));
    assert!(!is_literal("   "));
    assert!(is_sensitive_header(" authorization "));
    assert!(is_sensitive_header("X-Api-Key"));
    assert!(!is_sensitive_header("Accept"));
}
