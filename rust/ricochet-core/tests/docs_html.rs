use pretty_assertions::assert_eq;
use ricochet_core::docs_html::{MASKED, render};
use ricochet_core::models::*;

fn request_item(name: &str, url: &str) -> CollectionItem {
    request_item_with(name, url, |_| {})
}

fn request_item_with(name: &str, url: &str, configure: impl FnOnce(&mut ApiRequest)) -> CollectionItem {
    let mut request = ApiRequest { name: name.into(), url: url.into(), ..Default::default() };
    configure(&mut request);
    CollectionItem { name: name.into(), request: Some(request), ..Default::default() }
}

fn folder(name: &str, children: Vec<CollectionItem>) -> CollectionItem {
    CollectionItem { name: name.into(), children, ..Default::default() }
}

fn single(items: Vec<CollectionItem>) -> Collection {
    Collection { name: "Pets".into(), items, ..Default::default() }
}

fn secret(key: &str, value: &str) -> KeyValue {
    KeyValue { is_secret: true, ..KeyValue::new(key, value) }
}

/// How many folder sections are open at `position`.
fn open_sections_at(html: &str, position: usize) -> usize {
    let before = &html[..position];
    before.matches("<section class=\"folder\">").count() - before.matches("</section>").count()
}

#[test]
fn escapes_names_urls_headers_and_bodies() {
    let html = render(&single(vec![request_item_with("<script>alert(1)</script>", "https://x.test/?q=\"a\"&b='c'", |r| {
        r.headers = vec![KeyValue::new("X-Note", "<img src=x onerror=alert(1)>")];
        r.body = "{\"a\": \"</pre><script>\"}".into();
    })]));

    assert!(!html.contains("<script"));
    assert!(!html.contains("<img"));
    assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(html.contains("&lt;img src=x onerror=alert(1)&gt;"));
    assert!(html.contains("&lt;/pre&gt;&lt;script&gt;"));
    assert!(html.contains("q=&quot;a&quot;&amp;b=&#39;c&#39;"));
}

#[test]
fn escapes_html_in_the_collection_name() {
    let html = render(&Collection { name: "A & <B>".into(), ..Default::default() });

    assert!(html.contains("<title>A &amp; &lt;B&gt;</title>"));
    assert!(html.contains("<h1>A &amp; &lt;B&gt;</h1>"));
}

#[test]
fn secret_variable_values_are_not_written() {
    let collection = Collection {
        name: "Pets".into(),
        variables: vec![
            KeyValue::new("baseUrl", "https://api.test"),
            secret("apiSecret", "hunter2"),
            secret("emptySecret", ""),
            KeyValue { enabled: false, ..secret("disabledSecret", "leaked") },
        ],
        ..Default::default()
    };
    let html = render(&collection);

    assert!(!html.contains("hunter2"));
    assert!(!html.contains("leaked"));
    assert!(html.contains("https://api.test"));
    assert!(html.contains("apiSecret"));
    assert!(html.contains(MASKED));
}

#[test]
fn literal_credentials_are_not_written_and_placeholders_are_kept() {
    let html = render(&single(vec![
        request_item_with("Login", "https://x.test/", |r| {
            r.auth = Some(RequestAuth {
                kind: auth_kinds::BASIC.into(),
                username: "alice".into(),
                password: "s3cret-pass".into(),
                ..Default::default()
            });
        }),
        request_item_with("Bearer", "https://x.test/", |r| {
            r.auth = Some(RequestAuth {
                kind: auth_kinds::BEARER.into(),
                token: "{{apiToken}}".into(),
                ..Default::default()
            });
        }),
        request_item_with("ApiKey", "https://x.test/", |r| {
            r.auth = Some(RequestAuth {
                kind: auth_kinds::API_KEY.into(),
                key_name: "X-Key".into(),
                key_value: "key-abc-123".into(),
                ..Default::default()
            });
        }),
    ]));

    assert!(!html.contains("s3cret-pass"));
    assert!(!html.contains("key-abc-123"));
    assert!(html.contains("alice"));
    assert!(html.contains("{{apiToken}}"));
    assert!(html.contains("X-Key"));
}

#[test]
fn oauth_signing_secrets_are_not_written() {
    let html = render(&single(vec![request_item_with("Signed", "https://x.test/", |r| {
        r.auth = Some(RequestAuth {
            kind: auth_kinds::OAUTH1.into(),
            details: Some(AuthDetails {
                consumer_secret: "consumer-secret-value".into(),
                token_secret: "token-secret-value".into(),
                ..Default::default()
            }),
            ..Default::default()
        });
    })]));

    assert!(!html.contains("consumer-secret-value"));
    assert!(!html.contains("token-secret-value"));
}

#[test]
fn masks_sensitive_headers_and_secret_header_values() {
    let html = render(&single(vec![request_item_with("Get", "https://x.test/", |r| {
        r.headers = vec![
            KeyValue::new("Authorization", "Bearer abc"),
            KeyValue::new("Cookie", "{{session}}"),
            KeyValue::new("X-Custom", "visible-value"),
            secret("X-Hidden", "secret-header"),
            KeyValue { enabled: false, ..KeyValue::new("X-Api-Key", "off") },
        ];
    })]));

    assert!(!html.contains("Bearer abc"));
    assert!(!html.contains("secret-header"));
    assert!(!html.contains("X-Api-Key"));
    assert!(html.contains("{{session}}"));
    assert!(html.contains("visible-value"));
}

#[test]
fn masks_sensitive_query_parameters_in_the_url() {
    let html = render(&single(vec![request_item(
        "Search",
        "https://x.test/items?api_key=abc123&page=2&token={{ok}}",
    )]));

    assert!(!html.contains("abc123"));
    assert!(html.contains("api_key=***"));
    assert!(html.contains("page=2"));
    assert!(html.contains("token={{ok}}"));
}

#[test]
fn nests_requests_inside_their_folders() {
    let html = render(&single(vec![
        folder("Outer", vec![folder("Inner", vec![request_item("Deep request", "https://x.test/deep")])]),
        request_item("Top request", "https://x.test/top"),
    ]));

    let deep_at = html.find("Deep request").expect("deep request is written");
    let top_at = html.find("Top request").expect("top request is written");

    assert_eq!(open_sections_at(&html, deep_at), 2);
    assert_eq!(open_sections_at(&html, top_at), 0);
    assert!(html.contains("<h2>Outer</h2>"));
    assert!(html.contains("<h3>Inner</h3>"));
    assert!(html.contains("<h4><span class=\"method\">GET</span> Deep request</h4>"));
}

#[test]
fn clamps_heading_levels_for_deep_folders() {
    let mut leaf = request_item("Leaf", "https://x.test/");
    for i in 0..6 {
        leaf = folder(&format!("Level{i}"), vec![leaf]);
    }

    let html = render(&single(vec![leaf]));

    assert!(html.contains("<h2>Level5</h2>"));
    assert!(html.contains("<h6>Level1</h6>"));
    assert!(html.contains("<h6>Level0</h6>"));
    assert!(html.contains("<h6><span class=\"method\">GET</span> Leaf</h6>"));
}

#[test]
fn leaves_out_hidden_folders_and_requests() {
    let hidden_request = request_item("Hidden request", "https://x.test/hidden");
    let hidden_folder = folder("Hidden folder", vec![request_item("Inside hidden", "https://x.test/inside")]);
    let collection = Collection {
        name: "Pets".into(),
        hidden_item_ids: Some(vec![hidden_request.id.clone(), hidden_folder.id.clone()]),
        items: vec![request_item("Shown", "https://x.test/shown"), hidden_request, hidden_folder],
        ..Default::default()
    };

    let html = render(&collection);

    assert!(html.contains("Shown"));
    assert!(!html.contains("Hidden request"));
    assert!(!html.contains("Hidden folder"));
    assert!(!html.contains("Inside hidden"));
}

#[test]
fn page_has_no_scripts_external_resources_or_network_calls() {
    let html = render(&single(vec![folder("Folder", vec![request_item("Get", "https://api.example.test/v1/pets")])]));
    let lower = html.to_ascii_lowercase();

    assert!(!lower.contains("<script"));
    assert!(!lower.contains("<link"));
    assert!(!lower.contains("<img"));
    assert!(!lower.contains("<iframe"));
    assert!(!lower.contains("@import"));
    assert!(!lower.contains("url("));
    assert!(!lower.contains(" src="));
    assert!(!lower.contains(" href="));
    assert_eq!(html.matches("<style>").count(), 1);
    assert!(html.starts_with("<!DOCTYPE html>"));
}

#[test]
fn rendering_is_deterministic() {
    let collection = single(vec![folder("F", vec![request_item("R", "https://x.test/")])]);

    assert_eq!(render(&collection), render(&collection));
}
