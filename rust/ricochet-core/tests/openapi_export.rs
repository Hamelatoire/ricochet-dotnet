use pretty_assertions::assert_eq;
use ricochet_core::models::*;
use ricochet_core::openapi_export::{categories, export, ExportNote, ExportResult};
use serde_json::Value;

fn request(name: &str, method: &str, url: &str) -> ApiRequest {
    ApiRequest { name: name.into(), method: method.into(), url: url.into(), ..Default::default() }
}

fn item(name: &str, request: ApiRequest) -> CollectionItem {
    CollectionItem { name: name.into(), request: Some(request), ..Default::default() }
}

fn folder(name: &str, children: Vec<CollectionItem>) -> CollectionItem {
    CollectionItem { name: name.into(), children, ..Default::default() }
}

fn bearer(token: &str) -> Option<RequestAuth> {
    Some(RequestAuth { kind: auth_kinds::BEARER.into(), token: token.into(), ..Default::default() })
}

fn secret(key: &str, value: &str) -> KeyValue {
    KeyValue { is_secret: true, ..KeyValue::new(key, value) }
}

/// A Collection with a Secret, a Folder holding two Requests, and the auth and headers the export should keep or drop.
fn sample() -> Collection {
    let get = ApiRequest {
        headers: vec![
            KeyValue::new("Accept", "application/json"),
            KeyValue { enabled: false, ..KeyValue::new("X-Trace", "off") },
        ],
        auth: bearer("{{token}}"),
        ..request("Get product", "GET", "https://shop.example.test/products/{{productId}}?limit=10")
    };
    let create = ApiRequest {
        headers: vec![KeyValue::new("Content-Type", "application/json")],
        body: r#"{ "name": "Tea" }"#.into(),
        auth: bearer("{{token}}"),
        ..request("Create product", "POST", "https://shop.example.test/products")
    };

    Collection {
        name: "Shop".into(),
        variables: vec![secret("apiToken", "s3cr3t-value")],
        items: vec![folder("Products", vec![item("Get product", get), item("Create product", create)])],
        ..Default::default()
    }
}

fn parse(result: &ExportResult) -> Value {
    serde_json::from_str(&result.json).expect("the export is valid JSON")
}

fn has_note(result: &ExportResult, category: &str, location_suffix: &str, detail_part: &str) -> bool {
    result.notes.iter().any(|n: &ExportNote| {
        n.category == category && n.location.ends_with(location_suffix) && n.detail.contains(detail_part)
    })
}

#[test]
fn writes_an_openapi_3_document_with_one_operation_per_request() {
    let result = export(&sample());
    let doc = parse(&result);

    assert_eq!(doc["openapi"], "3.0.3");
    assert_eq!(doc["info"]["title"], "Shop");
    assert_eq!(doc["info"]["version"], "1.0.0");
    assert_eq!(doc["servers"][0]["url"], "https://shop.example.test");

    let get = &doc["paths"]["/products/{productId}"]["get"];
    assert_eq!(get["operationId"], "getproduct");
    assert_eq!(get["summary"], "Get product");
    assert_eq!(get["tags"][0], "Products");
    assert_eq!(get["security"][0]["bearerAuth"], serde_json::json!([]));

    let parameters: Vec<(String, String, bool)> = get["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["name"].as_str().unwrap().to_string(),
                p["in"].as_str().unwrap().to_string(),
                p["required"].as_bool().unwrap(),
            )
        })
        .collect();
    assert!(parameters.contains(&("productId".into(), "path".into(), true)));
    assert!(parameters.contains(&("limit".into(), "query".into(), false)));
    assert!(parameters.contains(&("Accept".into(), "header".into(), false)));
    assert!(!parameters.iter().any(|(name, _, _)| name == "X-Trace"), "disabled headers are left out");

    let post = &doc["paths"]["/products"]["post"];
    assert_eq!(post["requestBody"]["content"]["application/json"]["example"], r#"{ "name": "Tea" }"#);
    assert_eq!(post["requestBody"]["content"]["application/json"]["schema"]["type"], "string");

    let scheme = &doc["components"]["securitySchemes"]["bearerAuth"];
    assert_eq!(scheme["type"], "http");
    assert_eq!(scheme["scheme"], "bearer");
}

#[test]
fn top_level_keys_are_written_in_the_csharp_order() {
    let doc = parse(&export(&sample()));
    let keys: Vec<&str> = doc.as_object().unwrap().keys().map(String::as_str).collect();

    assert_eq!(keys, ["openapi", "info", "servers", "paths", "components"]);
}

#[test]
fn no_secret_value_or_sensitive_header_appears_in_the_document() {
    let mut collection = sample();
    collection.items.push(item(
        "Secure",
        ApiRequest {
            headers: vec![
                KeyValue::new("Authorization", "Bearer abc123-literal"),
                KeyValue::new("X-Token", "{{apiToken}}"),
            ],
            auth: Some(RequestAuth {
                kind: auth_kinds::BASIC.into(),
                username: "ann".into(),
                password: "pw-literal-42".into(),
                ..Default::default()
            }),
            ..request("Secure", "GET", "https://shop.example.test/secure?key={{apiToken}}")
        },
    ));

    let result = export(&collection);

    assert!(!result.json.contains("s3cr3t-value"));
    assert!(!result.json.contains("abc123-literal"));
    assert!(!result.json.contains("pw-literal-42"));
    assert!(!result.json.contains("\"Authorization\""));

    let doc = parse(&result);
    assert_eq!(doc["paths"]["/secure"]["get"]["security"][0]["basicAuth"], serde_json::json!([]));
    assert_eq!(doc["components"]["securitySchemes"]["basicAuth"]["scheme"], "basic");

    assert!(has_note(&result, categories::SECRET_VARIABLES, "Shop", "apiToken"));
    assert!(has_note(&result, categories::SECRET_VALUES, "Shop / Secure", "header Authorization"));
    assert!(has_note(&result, categories::SECRET_VALUES, "Shop / Secure", "basic password"));
}

#[test]
fn a_typed_bearer_token_is_left_out_and_the_scheme_kept() {
    let collection = Collection {
        name: "Typed".into(),
        items: vec![item(
            "Typed token",
            ApiRequest { auth: bearer("live-token-777"), ..request("Typed token", "GET", "https://api.example.test/me") },
        )],
        ..Default::default()
    };

    let result = export(&collection);

    assert!(!result.json.contains("live-token-777"));
    assert_eq!(parse(&result)["paths"]["/me"]["get"]["security"][0]["bearerAuth"], serde_json::json!([]));
    assert!(has_note(&result, categories::SECRET_VALUES, "Typed / Typed token", "bearer token was typed in as a value"));
}

#[test]
fn a_typed_api_key_is_left_out_and_its_scheme_names_the_key_location() {
    let collection = Collection {
        name: "Keys".into(),
        items: vec![item(
            "Query key",
            ApiRequest {
                auth: Some(RequestAuth {
                    kind: auth_kinds::API_KEY.into(),
                    key_name: "api_key".into(),
                    key_value: "k-literal-99".into(),
                    key_in_query: true,
                    ..Default::default()
                }),
                ..request("Query key", "GET", "https://api.example.test/data")
            },
        )],
        ..Default::default()
    };

    let result = export(&collection);
    let doc = parse(&result);

    assert!(!result.json.contains("k-literal-99"));
    let scheme = &doc["components"]["securitySchemes"]["apiKey_api_key_query"];
    assert_eq!(scheme["type"], "apiKey");
    assert_eq!(scheme["name"], "api_key");
    assert_eq!(scheme["in"], "query");
    assert_eq!(doc["paths"]["/data"]["get"]["security"][0]["apiKey_api_key_query"], serde_json::json!([]));
    assert!(has_note(&result, categories::SECRET_VALUES, "Keys / Query key", "API key api_key was typed in as a value"));
}

#[test]
fn a_secret_value_in_a_body_is_replaced_by_its_placeholder() {
    let collection = Collection {
        name: "Body".into(),
        variables: vec![secret("apiToken", "s3cr3t-value")],
        items: vec![item(
            "Send",
            ApiRequest {
                body: r#"{"key":"s3cr3t-value"}"#.into(),
                headers: vec![KeyValue::new("Content-Type", "application/json; charset=utf-8")],
                ..request("Send", "PUT", "https://api.example.test/send")
            },
        )],
        ..Default::default()
    };

    let result = export(&collection);
    let doc = parse(&result);

    assert!(!result.json.contains("s3cr3t-value"));
    let media = &doc["paths"]["/send"]["put"]["requestBody"]["content"]["application/json"];
    assert_eq!(media["example"], r#"{"key":"{{apiToken}}"}"#);
    assert!(has_note(&result, categories::SECRET_VALUES, "Body / Send body", "replaced with {{apiToken}}"));
}

#[test]
fn a_secret_variable_without_a_value_is_reported_as_having_none() {
    let collection = Collection {
        name: "Empty".into(),
        variables: vec![secret("apiToken", "")],
        ..Default::default()
    };

    let result = export(&collection);

    assert!(has_note(&result, categories::SECRET_VARIABLES, "Empty", "apiToken (has no value on this machine)"));
}

#[test]
fn plain_variables_are_reported_as_not_written() {
    let collection = Collection {
        name: "Plain".into(),
        variables: vec![KeyValue::new("host", "api.example.test")],
        ..Default::default()
    };

    let result = export(&collection);

    assert!(has_note(
        &result,
        categories::NOT_EXPORTED,
        "Plain",
        "1 variable (OpenAPI has no variables, so their values are not written)"
    ));
    assert!(!result.json.contains("api.example.test"));
}

#[test]
fn a_base_address_held_in_a_variable_is_reported_and_the_server_set_by_hand() {
    let collection = Collection {
        name: "Relative".into(),
        items: vec![item("Items", request("Items", "GET", "{{baseUrl}}/items"))],
        ..Default::default()
    };

    let result = export(&collection);
    let doc = parse(&result);

    assert_eq!(doc["servers"][0]["url"], "http://localhost");
    assert_eq!(doc["servers"][0]["description"], "Replace with the real server address.");
    assert!(doc["paths"].get("/items").is_some());
    assert!(has_note(&result, categories::NOT_EXPORTED, "Relative", "requests start with {{baseUrl}}"));
    assert!(has_note(&result, categories::NOT_EXPORTED, "Relative", "the server is set to http://localhost"));
}

#[test]
fn scripts_assertions_and_unsupported_methods_are_reported() {
    let odd = ApiRequest {
        assertions: Some(vec![ricochet_core::assertions::RequestAssertion { expected: "200".into(), ..Default::default() }]),
        ..request("Odd", "PROPFIND", "https://shop.example.test/odd")
    };
    let collection = Collection {
        name: "Shop".into(),
        items: vec![CollectionItem {
            scripts: vec![StoredScript { listen: "test".into(), source: "pm.test('x', () => {});".into() }],
            ..item("Odd", odd)
        }],
        ..Default::default()
    };

    let result = export(&collection);
    let doc = parse(&result);

    assert!(has_note(&result, categories::NOT_EXPORTED, "Shop / Odd", "the method PROPFIND has no OpenAPI equivalent"));
    assert!(has_note(&result, categories::NOT_EXPORTED, "Shop / Odd", "1 script (OpenAPI has no place for scripts)"));
    assert!(has_note(&result, categories::NOT_EXPORTED, "Shop / Odd", "1 assertion (OpenAPI has no tests)"));
    assert!(doc["paths"].get("/odd").is_none());
}

#[test]
fn a_second_request_with_the_same_method_and_path_is_reported_and_left_out() {
    let collection = Collection {
        name: "Dupes".into(),
        items: vec![
            item("List", request("List", "GET", "https://api.example.test/items")),
            item("List all", request("List all", "GET", "https://api.example.test/items?page=2")),
        ],
        ..Default::default()
    };

    let result = export(&collection);
    let doc = parse(&result);

    assert_eq!(doc["paths"]["/items"]["get"]["summary"], "List");
    assert!(has_note(
        &result,
        categories::NOT_EXPORTED,
        "Dupes / List all",
        "the same method and path as an earlier request, which OpenAPI cannot hold twice"
    ));
}

#[test]
fn operation_ids_are_made_unique_with_a_number() {
    let collection = Collection {
        name: "Ids".into(),
        items: vec![
            item("Get item", request("Get item", "GET", "https://api.example.test/a")),
            item("Get item", request("Get item", "GET", "https://api.example.test/b")),
            item("Get-item!", request("Get-item!", "GET", "https://api.example.test/c")),
            item("!!!", request("!!!", "GET", "https://api.example.test/d")),
        ],
        ..Default::default()
    };

    let doc = parse(&export(&collection));

    assert_eq!(doc["paths"]["/a"]["get"]["operationId"], "getitem");
    assert_eq!(doc["paths"]["/b"]["get"]["operationId"], "getitem2");
    assert_eq!(doc["paths"]["/c"]["get"]["operationId"], "getitem3");
    assert_eq!(doc["paths"]["/d"]["get"]["operationId"], "request");
}

#[test]
fn a_body_without_a_content_type_is_sent_as_json_or_text_by_its_first_character() {
    let collection = Collection {
        name: "Bodies".into(),
        items: vec![
            item("Array", ApiRequest { body: "[1, 2]".into(), ..request("Array", "POST", "https://api.example.test/a") }),
            item("Words", ApiRequest { body: "hello".into(), ..request("Words", "POST", "https://api.example.test/b") }),
        ],
        ..Default::default()
    };

    let doc = parse(&export(&collection));

    assert!(doc["paths"]["/a"]["post"]["requestBody"]["content"].get("application/json").is_some());
    assert!(doc["paths"]["/b"]["post"]["requestBody"]["content"].get("text/plain").is_some());
}

#[test]
fn a_request_with_no_full_address_gets_the_default_server() {
    let collection = Collection {
        name: "Local".into(),
        items: vec![item("Path only", request("Path only", "GET", "/status"))],
        ..Default::default()
    };

    let result = export(&collection);
    let doc = parse(&result);

    assert_eq!(doc["servers"][0]["url"], "http://localhost");
    assert!(doc["paths"].get("/status").is_some());
    assert!(has_note(&result, categories::NOT_EXPORTED, "Local", "no request has a full address"));
}

#[test]
fn path_and_query_parameters_are_named_from_the_url() {
    let collection = Collection {
        name: "Params".into(),
        items: vec![item(
            "Search",
            request("Search", "GET", "https://api.example.test/users/{{userId}}/posts?q=x&page={{page}}&q=y#top"),
        )],
        ..Default::default()
    };

    let doc = parse(&export(&collection));
    let parameters = doc["paths"]["/users/{userId}/posts"]["get"]["parameters"].as_array().unwrap();

    let names: Vec<&str> = parameters.iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["userId", "q", "page"]);
    assert_eq!(parameters[0]["in"], "path");
    assert_eq!(parameters[0]["required"], true);
}

#[test]
fn a_request_in_a_nested_folder_is_tagged_with_its_own_folder() {
    let collection = Collection {
        name: "Nested".into(),
        items: vec![folder(
            "Outer",
            vec![folder("Inner", vec![item("Ping", request("Ping", "GET", "https://api.example.test/ping"))])],
        )],
        ..Default::default()
    };

    let doc = parse(&export(&collection));

    assert_eq!(doc["paths"]["/ping"]["get"]["tags"][0], "Inner");
}

#[test]
fn the_report_groups_notes_by_category_in_first_seen_order() {
    let mut collection = sample();
    collection.variables.push(KeyValue::new("host", "api.example.test"));

    let report = export(&collection).report_text();

    assert_eq!(
        report,
        "Secret Variables left out (1):\n  Shop: apiToken\n\n\
         Not exported (1):\n  Shop: 1 variable (OpenAPI has no variables, so their values are not written)"
    );
}
