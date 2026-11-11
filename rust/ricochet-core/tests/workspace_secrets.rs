//! Port of `SecretRulesTests` and `WorkspaceSecretsTests` (tests/Ricochet.Core.Tests/SecretTests.cs). The tests that
//! wrote Workspace folders in C# check the same thing here by serialising the clean Workspace to JSON.

use std::collections::BTreeMap;

use chrono::DateTime;
use pretty_assertions::assert_eq;
use ricochet_core::models::{ApiEnvironment, ApiRequest, AuthDetails, Collection, CollectionItem, HistoryEntry, KeyValue, RequestAuth, Workspace};
use ricochet_core::secrets::SecretValue;
use ricochet_core::variables;
use ricochet_core::workspace_secrets::{
    HIDDEN_VALUE, PERSONAL_PREFIX, find_likely_literals, for_workspace, has_hidden_values, is_literal, merge, needs_migration,
    sanitize_history, split, with_workspace,
};

fn secret(key: &str, value: &str) -> KeyValue {
    KeyValue { is_secret: true, ..KeyValue::new(key, value) }
}

fn request_with_auth(auth: RequestAuth) -> ApiRequest {
    ApiRequest { id: "r1".into(), name: "R".into(), url: "u".into(), auth: Some(auth), ..ApiRequest::default() }
}

fn bearer(token: &str) -> RequestAuth {
    RequestAuth { kind: "bearer".into(), token: token.into(), ..RequestAuth::default() }
}

fn sample() -> Workspace {
    Workspace {
        collections: vec![Collection {
            id: "c1".into(),
            variables: vec![secret("csecret", "c-value"), KeyValue::new("plain", "p")],
            items: vec![CollectionItem {
                name: "Folder".into(),
                children: vec![CollectionItem {
                    name: "R".into(),
                    request: Some(request_with_auth(bearer("literal-token"))),
                    ..CollectionItem::default()
                }],
                ..CollectionItem::default()
            }],
            ..Collection::default()
        }],
        environments: vec![ApiEnvironment { id: "e1".into(), variables: vec![secret("esecret", "e-value")], ..ApiEnvironment::default() }],
        globals: vec![secret("gsecret", "g-value")],
        ..Workspace::default()
    }
}

fn history_entry(request: ApiRequest) -> HistoryEntry {
    HistoryEntry {
        request,
        status_code: Some(200),
        elapsed_ms: 1,
        at: DateTime::UNIX_EPOCH.fixed_offset(),
        collection_id: None,
        certificates_unverified: None,
    }
}

fn plain(entries: &BTreeMap<String, SecretValue>) -> Vec<&str> {
    entries.values().map(|value| value.expose()).collect()
}

#[test]
fn only_placeholders_are_safe_to_share() {
    for (value, expected) in [("abc", true), ("Bearer {{token}}", true), ("{{token}}", false), (" {{a}}{{b}} ", false), ("", false)] {
        assert_eq!(is_literal(value), expected, "{value:?}");
    }
}

#[test]
fn finds_credentials_in_headers_and_urls() {
    let request = ApiRequest {
        url: "http://x.test/?api_key=abc&token={{ok}}&page=2".into(),
        headers: vec![
            KeyValue::new("authorization", "Bearer abc"),
            KeyValue::new("Cookie", "{{ok}}"),
            KeyValue { enabled: false, ..KeyValue::new("X-Api-Key", "off") },
            KeyValue::new("Accept", "text/plain"),
        ],
        ..ApiRequest::default()
    };

    assert_eq!(find_likely_literals(&request), vec!["header authorization", "URL parameter api_key"]);
}

#[test]
fn split_removes_every_secret_and_merge_restores_them() {
    let (clean, secrets) = split(&sample());

    // What would be written to disk must not hold any of the values.
    let written = serde_json::to_string(&clean).unwrap();
    for value in ["c-value", "e-value", "g-value", "literal-token"] {
        assert!(!written.contains(value), "{value} leaked into the file");
        assert!(plain(&secrets).contains(&value), "{value} missing from the vault");
    }
    assert!(written.contains("\"plain\""));
    assert!(written.contains("csecret"));

    let merged = merge(&clean, &secrets);
    assert_eq!(merged.collections[0].variables[0].value, "c-value");
    assert!(merged.collections[0].variables[0].is_secret);
    assert_eq!(merged.collections[0].items[0].children[0].request.as_ref().unwrap().auth.as_ref().unwrap().token, "literal-token");
    assert_eq!(merged.environments[0].variables[0].value, "e-value");
    assert_eq!(merged.globals[0].value, "g-value");
}

#[test]
fn placeholders_in_auth_stay_in_the_file() {
    let workspace = Workspace {
        collections: vec![Collection {
            items: vec![CollectionItem { request: Some(request_with_auth(bearer("{{token}}"))), ..CollectionItem::default() }],
            ..Collection::default()
        }],
        ..Workspace::default()
    };

    let (clean, secrets) = split(&workspace);

    assert!(secrets.is_empty());
    assert_eq!(clean.collections[0].items[0].request.as_ref().unwrap().auth.as_ref().unwrap().token, "{{token}}");
}

#[test]
fn same_variable_name_in_different_scopes_keeps_separate_values() {
    let workspace = Workspace {
        environments: vec![
            ApiEnvironment { id: "staging".into(), variables: vec![secret("token", "s")], ..ApiEnvironment::default() },
            ApiEnvironment { id: "prod".into(), variables: vec![secret("token", "p")], ..ApiEnvironment::default() },
        ],
        ..Workspace::default()
    };

    let (clean, secrets) = split(&workspace);
    let merged = merge(&clean, &secrets);

    assert_eq!(merged.environments[0].variables[0].value, "s");
    assert_eq!(merged.environments[1].variables[0].value, "p");
}

#[test]
fn removed_secret_is_gone_from_the_next_split() {
    let (_, secrets) = split(&Workspace { globals: vec![secret("a", "1")], ..Workspace::default() });
    assert_eq!(secrets.len(), 1);

    let (_, after) = split(&Workspace::default());
    assert!(after.is_empty());
}

#[test]
fn a_value_found_in_the_file_wins_over_the_vault() {
    let clean = Workspace { globals: vec![secret("a", "from-file")], ..Workspace::default() };
    let mut vault = BTreeMap::new();
    vault.insert("variable/globals/a".to_string(), SecretValue::new("from-vault"));

    let merged = merge(&clean, &vault);

    assert_eq!(merged.globals[0].value, "from-file");
}

#[test]
fn secret_missing_on_this_machine_stays_empty_and_reports_as_unresolved() {
    let clean = Workspace { globals: vec![secret("token", "")], ..Workspace::default() };

    let merged = merge(&clean, &BTreeMap::new());
    assert_eq!(merged.globals[0].value, "");

    let resolved = variables::apply(&ApiRequest { url: "{{token}}".into(), ..ApiRequest::default() }, &merged.globals);
    assert_eq!(variables::find_placeholders(&resolved), vec!["token"]);
}

#[test]
fn history_hides_credentials_but_keeps_placeholders() {
    let entry = history_entry(ApiRequest {
        url: "u".into(),
        headers: vec![KeyValue::new("Authorization", "Bearer abc"), KeyValue::new("Cookie", "{{cookie}}"), KeyValue::new("Accept", "x")],
        auth: Some(RequestAuth { kind: "basic".into(), username: "me".into(), password: "pw".into(), ..RequestAuth::default() }),
        ..ApiRequest::default()
    });

    let request = sanitize_history(&entry).request;

    assert_eq!(request.headers[0].value, HIDDEN_VALUE);
    assert_eq!(request.headers[1].value, "{{cookie}}");
    assert_eq!(request.headers[2].value, "x");
    let auth = request.auth.as_ref().unwrap();
    assert_eq!(auth.username, "me");
    assert_eq!(auth.password, "");
    assert!(has_hidden_values(&request));
}

#[test]
fn split_also_cleans_history_already_on_disk() {
    let workspace = Workspace {
        history: vec![history_entry(ApiRequest {
            headers: vec![KeyValue::new("Authorization", "old-token")],
            ..ApiRequest::default()
        })],
        ..Workspace::default()
    };

    assert!(needs_migration(&workspace));
    let (clean, _) = split(&workspace);
    assert_eq!(clean.history[0].request.headers[0].value, HIDDEN_VALUE);
    assert!(!needs_migration(&clean));
}

#[test]
fn plain_text_auth_from_an_older_file_needs_migration() {
    assert!(needs_migration(&sample()));
    let (clean, _) = split(&sample());
    assert!(!needs_migration(&clean));
}

#[test]
fn signing_auth_secrets_are_taken_out_and_restored() {
    let auth = RequestAuth {
        kind: "oauth1".into(),
        details: Some(AuthDetails {
            consumer_key: "public-key".into(),
            consumer_secret: "consumer-literal".into(),
            token_secret: "{{tokenSecret}}".into(),
            aws_secret_access_key: "aws-literal".into(),
            ..AuthDetails::default()
        }),
        ..RequestAuth::default()
    };
    let workspace = Workspace {
        collections: vec![Collection {
            items: vec![CollectionItem { request: Some(request_with_auth(auth)), ..CollectionItem::default() }],
            ..Collection::default()
        }],
        ..Workspace::default()
    };

    let (clean, secrets) = split(&workspace);
    let clean_auth = clean.collections[0].items[0].request.as_ref().unwrap().auth.as_ref().unwrap();
    let clean_details = clean_auth.details.as_ref().unwrap();
    assert_eq!(clean_details.consumer_key, "public-key", "keys are not secret");
    assert_eq!(clean_details.consumer_secret, "");
    assert_eq!(clean_details.aws_secret_access_key, "");
    assert_eq!(clean_details.token_secret, "{{tokenSecret}}", "placeholders stay in the file");

    let merged = merge(&clean, &secrets);
    let details = merged.collections[0].items[0].request.as_ref().unwrap().auth.as_ref().unwrap().details.as_ref().unwrap().clone();
    assert_eq!(details.consumer_secret, "consumer-literal");
    assert_eq!(details.aws_secret_access_key, "aws-literal");
}

#[test]
fn a_workspace_only_sees_its_own_secrets_and_personal_ones() {
    let mut vault = BTreeMap::new();
    vault.insert("variable/globals/legacy".to_string(), SecretValue::new("old"));
    vault.insert("workspace/w1/variable/globals/a".to_string(), SecretValue::new("one"));
    vault.insert("workspace/w2/variable/globals/a".to_string(), SecretValue::new("two"));
    vault.insert(format!("{PERSONAL_PREFIX}clientCertificatePassword"), SecretValue::new("personal"));

    let mine = for_workspace(&vault, "w1");
    assert_eq!(mine.get("variable/globals/a").map(|value| value.expose()), Some("one"));
    assert_eq!(mine.get("variable/globals/legacy").map(|value| value.expose()), Some("old"));
    assert!(!mine.contains_key(&format!("{PERSONAL_PREFIX}clientCertificatePassword")));
    assert_eq!(mine.len(), 2);

    let mut replacement = BTreeMap::new();
    replacement.insert("variable/globals/a".to_string(), SecretValue::new("new"));
    let saved = with_workspace(&vault, "w1", &replacement);
    assert_eq!(saved.get("workspace/w1/variable/globals/a").map(|value| value.expose()), Some("new"));
    assert_eq!(saved.get("workspace/w2/variable/globals/a").map(|value| value.expose()), Some("two"));
    assert_eq!(saved.get(&format!("{PERSONAL_PREFIX}clientCertificatePassword")).map(|value| value.expose()), Some("personal"));
}

#[test]
fn a_request_without_auth_or_literals_is_left_alone() {
    let entry = history_entry(ApiRequest { headers: vec![KeyValue::new("Accept", "text/plain")], ..ApiRequest::default() });
    assert_eq!(sanitize_history(&entry), entry);
}
