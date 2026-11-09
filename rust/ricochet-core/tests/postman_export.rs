use pretty_assertions::assert_eq;
use ricochet_core::models::*;
use ricochet_core::postman_export::{categories, export, ExportNote, SCHEMA};
use ricochet_core::assertions::{RequestAssertion, RequestCapture};
use serde_json::Value;

fn kv(key: &str, value: &str) -> KeyValue {
    KeyValue::new(key, value)
}

fn secret(key: &str, value: &str) -> KeyValue {
    KeyValue { is_secret: true, ..KeyValue::new(key, value) }
}

fn disabled(mut header: KeyValue) -> KeyValue {
    header.enabled = false;
    header
}

fn request(name: &str, method: &str, url: &str) -> ApiRequest {
    ApiRequest { name: name.into(), method: method.into(), url: url.into(), ..Default::default() }
}

fn leaf(name: &str, request: ApiRequest) -> CollectionItem {
    CollectionItem { name: name.into(), request: Some(request), ..Default::default() }
}

fn folder(name: &str, children: Vec<CollectionItem>) -> CollectionItem {
    CollectionItem { name: name.into(), children, ..Default::default() }
}

fn script(listen: &str, source: &str) -> StoredScript {
    StoredScript { listen: listen.into(), source: source.into() }
}

fn bearer(token: &str) -> Option<RequestAuth> {
    Some(RequestAuth { kind: auth_kinds::BEARER.into(), token: token.into(), ..Default::default() })
}

/// The shape the tests share: a folder with two Requests, a top-level Request with basic auth, and a
/// disabled Variable, header and script.
fn sample() -> Collection {
    let list = ApiRequest {
        headers: vec![kv("Accept", "application/json"), disabled(kv("X-Trace", "off"))],
        auth: bearer("{{token}}"),
        ..request("List", "GET", "{{base}}/products?limit=10")
    };
    let mut list_item = leaf("List", list);
    list_item.scripts = vec![script("test", "pm.test('ok', () => {});")];

    let create = ApiRequest {
        headers: vec![kv("Content-Type", "application/json")],
        body: r#"{ "name": "Tea", "price": 3 }"#.into(),
        auth: Some(RequestAuth {
            kind: auth_kinds::API_KEY.into(),
            key_name: "X-Key".into(),
            key_value: "{{apiKey}}".into(),
            key_in_query: true,
            ..Default::default()
        }),
        ..request("Create", "POST", "{{base}}/products")
    };

    let login = ApiRequest {
        headers: vec![kv("Content-Type", "application/x-www-form-urlencoded")],
        body: "user=a&pass=b".into(),
        auth: Some(RequestAuth {
            kind: auth_kinds::BASIC.into(),
            username: "{{user}}".into(),
            password: "{{pass}}".into(),
            ..Default::default()
        }),
        ..request("Login", "POST", "{{base}}/login")
    };

    Collection {
        name: "Shop".into(),
        variables: vec![kv("base", "https://shop.example.test"), KeyValue { enabled: false, ..kv("debug", "1") }],
        scripts: vec![script("prerequest", "console.log('start');")],
        items: vec![
            folder("Products", vec![list_item, leaf("Create", create)]),
            leaf("Login", login),
            leaf("Health", request("Health", "GET", "http://health.example.test/")),
        ],
        ..Default::default()
    }
}

fn parse(json: &str) -> Value {
    serde_json::from_str(json).expect("export is valid JSON")
}

/// Finds the Request at a path of item names, folders first.
fn request_at<'a>(root: &'a Value, path: &[&str]) -> &'a Value {
    let mut items = root["item"].as_array().expect("item array");
    for (index, name) in path.iter().enumerate() {
        let found = items
            .iter()
            .find(|item| item["name"] == *name)
            .unwrap_or_else(|| panic!("no item named {name}"));
        if index == path.len() - 1 {
            return &found["request"];
        }
        items = found["item"].as_array().expect("folder items");
    }

    panic!("empty path");
}

fn notes_in(notes: &[ExportNote], category: &str) -> Vec<(String, String)> {
    notes
        .iter()
        .filter(|n| n.category == category)
        .map(|n| (n.location.clone(), n.detail.clone()))
        .collect()
}

#[test]
fn writes_a_v21_collection_with_folders_and_requests() {
    let collection = sample();
    let result = export(&collection);
    let root = parse(&result.json);

    assert_eq!(root["info"]["schema"], SCHEMA);
    assert_eq!(root["info"]["name"], "Shop");
    assert_eq!(root["info"]["_postman_id"], collection.id.as_str());
    assert_eq!(root["item"][0]["name"], "Products");
    assert_eq!(root["item"][0]["item"][0]["request"]["method"], "GET");
    assert!(notes_in(&result.notes, categories::SECRET_VARIABLES).is_empty());
}

#[test]
fn exported_requests_keep_method_url_headers_body_and_auth() {
    let root = parse(&export(&sample()).json);

    let list = request_at(&root, &["Products", "List"]);
    assert_eq!(list["method"], "GET");
    assert_eq!(list["url"]["raw"], "{{base}}/products?limit=10");
    assert_eq!(list["header"][0], serde_json::json!({ "key": "Accept", "value": "application/json" }));
    assert_eq!(list["header"][1], serde_json::json!({ "key": "X-Trace", "value": "off", "disabled": true }));
    assert_eq!(list["auth"]["type"], "bearer");
    assert_eq!(list["auth"]["bearer"][0]["value"], "{{token}}");
    assert!(list.get("body").is_none(), "no body means no body key");

    let create = request_at(&root, &["Products", "Create"]);
    assert_eq!(create["body"]["mode"], "raw");
    assert_eq!(create["body"]["raw"], r#"{ "name": "Tea", "price": 3 }"#);
    assert_eq!(create["body"]["options"]["raw"]["language"], "json");
    assert_eq!(create["auth"]["type"], "apikey");
    assert_eq!(create["auth"]["apikey"][2], serde_json::json!({ "key": "in", "value": "query", "type": "string" }));

    let login = request_at(&root, &["Login"]);
    assert_eq!(login["body"]["options"]["raw"]["language"], "text");
    assert_eq!(login["auth"]["basic"][0], serde_json::json!({ "key": "username", "value": "{{user}}", "type": "string" }));
    assert_eq!(login["auth"]["basic"][1], serde_json::json!({ "key": "password", "value": "{{pass}}", "type": "string" }));
}

#[test]
fn variables_scripts_and_disabled_flags_are_written() {
    let root = parse(&export(&sample()).json);

    assert_eq!(root["variable"][0], serde_json::json!({ "key": "base", "value": "https://shop.example.test" }));
    assert_eq!(root["variable"][1], serde_json::json!({ "key": "debug", "value": "1", "disabled": true }));
    assert_eq!(root["event"][0]["listen"], "prerequest");
    assert_eq!(root["event"][0]["script"]["type"], "text/javascript");
    assert_eq!(root["event"][0]["script"]["exec"], serde_json::json!(["console.log('start');"]));
    assert_eq!(root["item"][0]["item"][0]["event"][0]["listen"], "test");
}

#[test]
fn optional_keys_are_left_out_when_empty() {
    let collection = Collection {
        name: "Bare".into(),
        items: vec![leaf("Ping", request("Ping", "get", "https://example.test/ping"))],
        ..Default::default()
    };
    let root = parse(&export(&collection).json);

    assert!(root.get("variable").is_none());
    assert!(root.get("event").is_none());
    let ping = request_at(&root, &["Ping"]);
    assert_eq!(ping["method"], "GET", "method is upper-cased");
    assert_eq!(ping["auth"], serde_json::json!({ "type": "noauth" }));
    assert!(ping.get("body").is_none());
}

#[test]
fn missing_and_unsupported_auth_are_written_as_noauth() {
    let digest = ApiRequest {
        auth: Some(RequestAuth { kind: auth_kinds::DIGEST.into(), username: "u".into(), ..Default::default() }),
        ..request("Digest", "GET", "https://example.test/d")
    };
    let collection = Collection {
        name: "Auth".into(),
        items: vec![leaf("Digest", digest)],
        ..Default::default()
    };
    let root = parse(&export(&collection).json);

    assert_eq!(request_at(&root, &["Digest"])["auth"], serde_json::json!({ "type": "noauth" }));
}

#[test]
fn a_secret_variable_is_left_out_and_its_references_stay_as_placeholders() {
    let mut collection = sample();
    collection.variables = vec![secret("apiToken", "s3cr3t-value"), kv("copy", "s3cr3t-value")];
    collection.items = vec![leaf(
        "Ping",
        ApiRequest { headers: vec![kv("X-Token", "{{apiToken}}")], ..request("Ping", "GET", "{{base}}/ping") },
    )];

    let result = export(&collection);

    assert!(!result.json.contains("s3cr3t-value"));
    let root = parse(&result.json);
    let variables = root["variable"].as_array().expect("variables");
    assert!(variables.iter().all(|v| v["key"] != "apiToken"));
    assert!(variables.iter().any(|v| v["key"] == "copy" && v["value"] == "{{apiToken}}"));
    assert_eq!(
        notes_in(&result.notes, categories::SECRET_VARIABLES),
        vec![("Shop".to_string(), "apiToken".to_string())]
    );
    let value_notes = notes_in(&result.notes, categories::SECRET_VALUES);
    assert!(value_notes.iter().any(|(_, detail)| detail.contains("{{apiToken}}")));
    assert!(result.json.contains("{{apiToken}}"));
}

#[test]
fn a_secret_variable_with_no_value_is_noted_as_such() {
    let collection = Collection {
        name: "Shop".into(),
        variables: vec![secret("apiToken", "")],
        ..Default::default()
    };
    let result = export(&collection);

    assert_eq!(
        notes_in(&result.notes, categories::SECRET_VARIABLES),
        vec![("Shop".to_string(), "apiToken (has no value on this machine)".to_string())]
    );
}

#[test]
fn typed_in_secrets_are_replaced_with_placeholders_and_reported() {
    let collection = Collection {
        name: "Shop".into(),
        items: vec![
            leaf(
                "Secure",
                ApiRequest {
                    headers: vec![kv("Authorization", "Bearer abc123-literal")],
                    auth: bearer("tok-literal-999"),
                    ..request("Secure", "GET", "https://example.test/secure")
                },
            ),
            leaf(
                "Basic",
                ApiRequest {
                    auth: Some(RequestAuth {
                        kind: auth_kinds::BASIC.into(),
                        username: "ann".into(),
                        password: "pw-literal-42".into(),
                        ..Default::default()
                    }),
                    ..request("Basic", "GET", "https://example.test/basic")
                },
            ),
        ],
        ..Default::default()
    };

    let result = export(&collection);

    for literal in ["abc123-literal", "tok-literal-999", "pw-literal-42"] {
        assert!(!result.json.contains(literal), "{literal} leaked into the export");
    }
    let root = parse(&result.json);
    assert_eq!(request_at(&root, &["Secure"])["header"][0]["value"], "{{Authorization}}");
    assert_eq!(request_at(&root, &["Secure"])["auth"]["bearer"][0]["value"], "{{token}}");
    assert_eq!(request_at(&root, &["Basic"])["auth"]["basic"][1]["value"], "{{password}}");
    assert_eq!(request_at(&root, &["Basic"])["auth"]["basic"][0]["value"], "ann", "a username is not a Secret");

    let typed = notes_in(&result.notes, categories::SECRET_VALUES);
    assert!(typed.contains(&(
        "Shop / Secure".to_string(),
        "header Authorization was typed in as a value, so {{Authorization}} is written instead. Set it as a Variable.".to_string()
    )));
    assert!(typed.iter().any(|(location, detail)| location == "Shop / Secure" && detail.starts_with("bearer token ")));
    assert!(typed.iter().any(|(location, detail)| location == "Shop / Basic" && detail.starts_with("basic password ")));
}

#[test]
fn a_typed_in_placeholder_is_kept_and_not_reported() {
    let collection = Collection {
        name: "Shop".into(),
        items: vec![leaf(
            "Templated",
            ApiRequest {
                headers: vec![kv("Authorization", "{{token}}")],
                ..request("Templated", "GET", "https://example.test/t")
            },
        )],
        ..Default::default()
    };
    let result = export(&collection);

    assert!(result.notes.is_empty());
    assert_eq!(request_at(&parse(&result.json), &["Templated"])["header"][0]["value"], "{{token}}");
}

#[test]
fn secret_values_are_not_written_into_a_script_or_body() {
    let collection = Collection {
        name: "Shop".into(),
        variables: vec![secret("apiToken", "s3cr3t-value")],
        scripts: vec![script("prerequest", "const t = 's3cr3t-value';")],
        items: vec![CollectionItem {
            scripts: vec![script("test", "s3cr3t-value")],
            ..leaf(
                "Send",
                ApiRequest { body: "token=s3cr3t-value".into(), ..request("Send", "POST", "{{base}}/x") },
            )
        }],
        ..Default::default()
    };

    let result = export(&collection);

    assert!(!result.json.contains("s3cr3t-value"));
    let root = parse(&result.json);
    assert_eq!(request_at(&root, &["Send"])["body"]["raw"], "token={{apiToken}}");
    assert_eq!(root["event"][0]["script"]["exec"], serde_json::json!(["const t = '{{apiToken}}';"]));
    assert_eq!(root["item"][0]["event"][0]["script"]["exec"], serde_json::json!(["{{apiToken}}"]));
}

#[test]
fn a_longer_secret_is_replaced_whole_before_a_shorter_one_inside_it() {
    let collection = Collection {
        name: "Shop".into(),
        variables: vec![secret("short", "hunter2"), secret("long", "hunter2-extended")],
        items: vec![leaf(
            "Url",
            request("Url", "GET", "https://example.test/?k=hunter2-extended&p=hunter2"),
        )],
        ..Default::default()
    };

    let result = export(&collection);

    assert!(!result.json.contains("hunter2"));
    assert_eq!(request_at(&parse(&result.json), &["Url"])["url"]["raw"], "https://example.test/?k={{long}}&p={{short}}");
}

#[test]
fn no_secret_value_appears_anywhere_in_the_export() {
    // alpha and beta are Secret Variables. gamma and delta are typed straight into Requests. ADR 0029 only
    // promises to catch these known Secrets, so every secret below is one of those two kinds.
    let alpha = "alpha-secret-111";
    let beta = "beta-secret-222";
    let gamma = "gamma-secret-333";
    let delta = "delta-secret-444";
    let collection = Collection {
        name: "Everything".into(),
        variables: vec![
            secret("alpha", alpha),
            kv("plain", &format!("contains {beta}")),
            secret("beta", beta),
        ],
        scripts: vec![script("prerequest", &format!("const x = '{alpha}';"))],
        items: vec![folder(
            "Folder",
            vec![
                leaf(
                    "Typed",
                    ApiRequest {
                        headers: vec![kv("Cookie", gamma), kv("X-Plain", alpha)],
                        body: format!("{beta} and more"),
                        auth: Some(RequestAuth {
                            kind: auth_kinds::API_KEY.into(),
                            key_name: "X-Api".into(),
                            key_value: delta.into(),
                            ..Default::default()
                        }),
                        assertions: Some(vec![RequestAssertion { expected: alpha.into(), ..Default::default() }]),
                        ..request("Typed", "POST", &format!("https://example.test/?t={beta}"))
                    },
                ),
                CollectionItem {
                    scripts: vec![script("test", alpha)],
                    ..folder("Empty", vec![])
                },
            ],
        )],
        ..Default::default()
    };

    let result = export(&collection);

    for secret_value in [alpha, beta, gamma, delta] {
        assert!(!result.json.contains(secret_value), "{secret_value} leaked into the export");
    }
    let root = parse(&result.json);
    let typed = request_at(&root, &["Folder", "Typed"]);
    assert_eq!(typed["header"][0]["value"], "{{Cookie}}");
    assert_eq!(typed["auth"]["apikey"][1]["value"], "{{X-Api}}");
    assert_eq!(typed["url"]["raw"], "https://example.test/?t={{beta}}");
}

#[test]
fn notes_say_what_postman_could_not_hold() {
    let collection = Collection {
        name: "Shop".into(),
        items: vec![leaf(
            "Checked",
            ApiRequest {
                assertions: Some(vec![RequestAssertion { expected: "200".into(), ..Default::default() }]),
                ..request("Checked", "GET", "{{base}}/checked")
            },
        )],
        ..Default::default()
    };

    let result = export(&collection);

    assert_eq!(
        notes_in(&result.notes, categories::NOT_EXPORTED),
        vec![("Shop / Checked".to_string(), "1 assertion (Postman tests are not written)".to_string())]
    );
    assert_eq!(
        result.to_report_text(),
        "Not exported (1):\n  Shop / Checked: 1 assertion (Postman tests are not written)"
    );
}

#[test]
fn captures_are_counted_in_the_report() {
    let collection = Collection {
        name: "Shop".into(),
        items: vec![leaf(
            "Capture",
            ApiRequest {
                captures: Some(vec![RequestCapture { variable: "a".into(), ..Default::default() }, RequestCapture { variable: "b".into(), ..Default::default() }]),
                ..request("Capture", "GET", "https://example.test/c")
            },
        )],
        ..Default::default()
    };

    let result = export(&collection);

    assert_eq!(
        notes_in(&result.notes, categories::NOT_EXPORTED),
        vec![("Shop / Capture".to_string(), "2 captures".to_string())]
    );
}

#[test]
fn report_groups_notes_in_the_order_categories_first_appear() {
    let collection = Collection {
        name: "Shop".into(),
        variables: vec![secret("token", "abc-value")],
        items: vec![leaf(
            "Mixed",
            ApiRequest {
                headers: vec![kv("Authorization", "Bearer typed-value")],
                captures: Some(vec![RequestCapture::default()]),
                ..request("Mixed", "GET", "{{base}}/m?x=abc-value")
            },
        )],
        ..Default::default()
    };

    let text = export(&collection).to_report_text();

    let expected = [
        "Secret Variables left out (1):",
        "  Shop: token",
        "",
        "Secret values kept out of the export (2):",
        "  Shop / Mixed: the value of secret token was replaced with {{token}}",
        "  Shop / Mixed: header Authorization was typed in as a value, so {{Authorization}} is written instead. Set it as a Variable.",
        "",
        "Not exported (1):",
        "  Shop / Mixed: 1 capture",
    ]
    .join("\n");
    assert_eq!(text, expected);
}
