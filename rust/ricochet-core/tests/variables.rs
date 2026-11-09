use pretty_assertions::assert_eq;
use ricochet_core::models::*;
use ricochet_core::variables::*;

fn var(key: &str, value: &str) -> KeyValue {
    KeyValue::new(key, value)
}

fn request(url: &str) -> ApiRequest {
    ApiRequest { url: url.into(), ..Default::default() }
}

#[test]
fn replaces_variables_in_url_and_headers() {
    let mut req = request("https://{{host}}/users");
    req.headers = vec![var("X-Env", "{{env}}")];
    let resolved = resolve(&req, &[var("host", "api.test"), var("env", "staging")]);

    assert_eq!(resolved.sent.url, "https://api.test/users");
    assert_eq!(resolved.sent.headers[0].value, "staging");
}

#[test]
fn history_keeps_variables_as_written() {
    let req = request("https://{{host}}/x");
    let resolved = resolve(&req, &[var("host", "api.test")]);

    assert_eq!(resolved.history.url, "https://{{host}}/x");
}

#[test]
fn unknown_names_are_left_as_written() {
    let req = request("https://{{missing}}/x");
    let resolved = resolve(&req, &[]);

    assert_eq!(resolved.sent.url, "https://{{missing}}/x");
    assert_eq!(resolved.history.url, "https://{{missing}}/x");
}

#[test]
fn nested_variables_are_expanded() {
    let req = request("{{a}}");
    let resolved = resolve(&req, &[var("a", "{{b}}/end"), var("b", "start")]);

    assert_eq!(resolved.sent.url, "start/end");
}

#[test]
fn a_variable_that_refers_to_itself_is_left_as_written() {
    let req = request("{{a}}");
    let resolved = resolve(&req, &[var("a", "{{b}}"), var("b", "{{a}}")]);

    assert_eq!(resolved.sent.url, "{{a}}");
}

#[test]
fn a_later_variable_overrides_an_earlier_one() {
    let scopes: [&[KeyValue]; 2] = [&[var("k", "global")], &[var("k", "collection")]];
    let variables = layer(&scopes);
    let resolved = resolve(&request("{{k}}"), &variables);

    assert_eq!(resolved.sent.url, "collection");
}

#[test]
fn a_disabled_variable_does_not_override_a_lower_scope() {
    let mut disabled = var("k", "override");
    disabled.enabled = false;
    let resolved = resolve(&request("{{k}}"), &[var("k", "base"), disabled]);

    assert_eq!(resolved.sent.url, "base");
}

#[test]
fn an_empty_secret_counts_as_unset() {
    let mut secret = var("token", "");
    secret.is_secret = true;
    let resolved = resolve(&request("{{token}}"), &[secret]);

    assert_eq!(resolved.sent.url, "{{token}}");
}

#[test]
fn json_body_escapes_values_inside_strings() {
    let mut req = request("");
    req.body = r#"{"name": "{{n}}"}"#.into();
    let resolved = resolve(&req, &[var("n", r#"a"b"#)]);

    assert_eq!(resolved.sent.body, r#"{"name": "a\"b"}"#);
    assert_eq!(resolved.history.body, r#"{"name": "{{n}}"}"#);
}

#[test]
fn json_body_inserts_values_outside_strings_as_written() {
    let mut req = request("");
    req.body = r#"{"obj": {{o}}}"#.into();
    let resolved = resolve(&req, &[var("o", r#"{"x":1}"#)]);

    assert_eq!(resolved.sent.body, r#"{"obj": {"x":1}}"#);
}

#[test]
fn bearer_token_is_resolved_and_kept_as_written_in_history() {
    let mut req = request("https://a.test");
    req.auth = Some(RequestAuth { kind: auth_kinds::BEARER.into(), token: "{{t}}".into(), ..Default::default() });
    let resolved = resolve(&req, &[var("t", "abc")]);

    assert_eq!(resolved.sent.auth.unwrap().token, "abc");
    assert_eq!(resolved.history.auth.unwrap().token, "{{t}}");
}

#[test]
fn placeholders_list_unique_names_in_order() {
    let mut req = request("{{a}}/{{b}}");
    req.body = "{{a}}".into();
    req.headers = vec![var("X", "{{c}}"), KeyValue { enabled: false, ..var("Y", "{{hidden}}") }];

    assert_eq!(find_placeholders(&req), vec!["a", "b", "c"]);
}

#[test]
fn auth_fields_not_used_by_the_chosen_kind_are_ignored() {
    let mut req = request("https://a.test");
    req.auth = Some(RequestAuth { kind: auth_kinds::NONE.into(), token: "{{t}}".into(), ..Default::default() });

    assert!(find_placeholders(&req).is_empty());
}

#[test]
fn an_unclosed_placeholder_is_reported() {
    let req = request("https://a.test/{{user");

    assert_eq!(find_placeholders(&req), vec!["{{user"]);
}

#[test]
fn folder_scope_is_outermost_first() {
    let request_item = CollectionItem {
        id: "r".into(),
        name: "R".into(),
        request: Some(request("https://a.test")),
        ..Default::default()
    };
    let inner = CollectionItem {
        name: "Inner".into(),
        children: vec![request_item],
        variables: vec![var("level", "inner")],
        ..Default::default()
    };
    let outer = CollectionItem {
        name: "Outer".into(),
        children: vec![inner],
        variables: vec![var("level", "outer"), var("only", "outer")],
        ..Default::default()
    };
    let collection = Collection { items: vec![outer], ..Default::default() };

    let scope = folder_scope(&collection, Some("r"));
    let keys: Vec<(&str, &str)> = scope.iter().map(|v| (v.key.as_str(), v.value.as_str())).collect();
    assert_eq!(keys, vec![("level", "outer"), ("only", "outer"), ("level", "inner")]);
}

#[test]
fn folder_scope_is_empty_for_a_request_directly_in_the_collection() {
    let direct = CollectionItem { id: "r".into(), request: Some(request("https://a.test")), ..Default::default() };
    let collection = Collection { items: vec![direct], ..Default::default() };

    assert!(folder_scope(&collection, Some("r")).is_empty());
    assert!(folder_scope(&collection, None).is_empty());
}

#[test]
fn prompt_variables_are_found_through_other_variables() {
    let mut prompt = var("p", "default");
    prompt.is_prompt = true;
    let variables = [var("x", "{{p}}"), prompt];

    let found = find_prompt_variables(&request("{{x}}"), &variables);
    assert_eq!(found, vec![PromptVariable { name: "p".into(), default: "default".into() }]);
}

#[test]
fn apply_returns_only_the_sent_request() {
    let applied = apply(&request("{{h}}"), &[var("h", "x.test")]);
    assert_eq!(applied.url, "x.test");
}
