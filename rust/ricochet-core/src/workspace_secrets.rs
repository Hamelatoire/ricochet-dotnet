//! Port of `WorkspaceSecrets.cs` and the parts of `SecretRules` it uses. Secrets are taken out of a Workspace before it is
//! saved (`split`), and put back after it is loaded (`merge`). Values are kept in the vault under keys that name their
//! owner, so each Secret has one place (ADR 0004, ADR 0092). The key scheme is the C# scheme, so both apps read the same
//! vault: `variable/{scope}/{name}`, `auth/{requestId}/{field}`, `workspace/{workspaceId}/{key}` and `personal/...`.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use regex::Regex;

use crate::models::{ApiRequest, CollectionItem, HistoryEntry, KeyValue, RequestAuth, Workspace};
use crate::secrets::{SecretValue, auth_key};

/// Replaces sensitive values in History. Plain ASCII, because it may end up in a header if replayed.
pub const HIDDEN_VALUE: &str = "[hidden]";

/// The scope of the Workspace's global Variables. Same name as the C# app.
const GLOBALS_SCOPE: &str = "globals";
/// The prefix of a Workspace's own Secrets in the vault. Same as the C# app.
const WORKSPACE_PREFIX: &str = "workspace/";
/// The prefix of personal Secrets, which belong to no Workspace. Same as `PersonalSecrets.Prefix` in C#.
pub const PERSONAL_PREFIX: &str = "personal/";

const SENSITIVE_HEADERS: [&str; 8] = [
    "Authorization",
    "Proxy-Authorization",
    "Cookie",
    "Set-Cookie",
    "X-Api-Key",
    "Api-Key",
    "X-Auth-Token",
    "X-Access-Token",
];

fn placeholder() -> &'static Regex {
    static PLACEHOLDER: OnceLock<Regex> = OnceLock::new();
    PLACEHOLDER.get_or_init(|| Regex::new(r"\{\{[^{}]+\}\}").expect("valid pattern"))
}

fn sensitive_query_parameter() -> &'static Regex {
    static SENSITIVE_QUERY_PARAMETER: OnceLock<Regex> = OnceLock::new();
    SENSITIVE_QUERY_PARAMETER.get_or_init(|| {
        Regex::new(r"(?i)[?&](api[_-]?key|access[_-]?token|auth[_-]?token|token|password|passwd|secret)=([^&#]+)")
            .expect("valid pattern")
    })
}

/// True when the text holds an actual value, not just `{{placeholders}}` that are safe to share.
pub fn is_literal(value: &str) -> bool {
    !placeholder().replace_all(value, "").trim().is_empty()
}

/// True for the headers that carry credentials. Names are not case-sensitive.
pub fn is_sensitive_header(name: &str) -> bool {
    let name = name.trim();
    SENSITIVE_HEADERS.iter().any(|known| known.eq_ignore_ascii_case(name))
}

/// Best-effort list of credentials written where they would be stored in plain text. Each entry names the place,
/// such as `header Authorization` or `URL parameter api_key`.
pub fn find_likely_literals(request: &ApiRequest) -> Vec<String> {
    let mut found = Vec::new();
    for header in &request.headers {
        if header.enabled && is_sensitive_header(&header.key) && is_literal(&header.value) && header.value != HIDDEN_VALUE {
            found.push(format!("header {}", header.key.trim()));
        }
    }
    for caps in sensitive_query_parameter().captures_iter(&request.url) {
        if is_literal(&caps[2]) {
            found.push(format!("URL parameter {}", &caps[1]));
        }
    }
    found
}

/// The Secrets of one Workspace, keyed as the Workspace's own files expect. Entries from before Workspaces had ids
/// count as belonging to it, so they are included too.
pub fn for_workspace(vault: &BTreeMap<String, SecretValue>, workspace_id: &str) -> BTreeMap<String, SecretValue> {
    let prefix = format!("{WORKSPACE_PREFIX}{workspace_id}/");
    let mut result = BTreeMap::new();
    for (key, value) in vault {
        if !key.starts_with(WORKSPACE_PREFIX) && !key.starts_with(PERSONAL_PREFIX) {
            result.insert(key.clone(), value.clone());
        }
    }
    for (key, value) in vault {
        if let Some(name) = key.strip_prefix(prefix.as_str()) {
            result.insert(name.to_string(), value.clone());
        }
    }
    result
}

/// Replaces one Workspace's Secrets, and leaves those of every other Workspace alone.
pub fn with_workspace(
    vault: &BTreeMap<String, SecretValue>,
    workspace_id: &str,
    secrets: &BTreeMap<String, SecretValue>,
) -> BTreeMap<String, SecretValue> {
    let prefix = format!("{WORKSPACE_PREFIX}{workspace_id}/");
    let mut result = BTreeMap::new();
    for (key, value) in vault {
        // Personal Secrets belong to no Workspace, so they survive a save of this one.
        let is_other_workspace = key.starts_with(WORKSPACE_PREFIX) && !key.starts_with(prefix.as_str());
        if is_other_workspace || key.starts_with(PERSONAL_PREFIX) {
            result.insert(key.clone(), value.clone());
        }
    }
    for (key, value) in secrets {
        result.insert(format!("{prefix}{key}"), value.clone());
    }
    result
}

/// Returns a copy safe to write to disk, plus the values that belong in the vault. History is cleaned too.
pub fn split(workspace: &Workspace) -> (Workspace, BTreeMap<String, SecretValue>) {
    let mut secrets = BTreeMap::new();
    let mut clean = workspace.clone();

    for collection in &mut clean.collections {
        collection.variables = split_variables(&collection.variables, &collection.id, &mut secrets);
        for item in &mut collection.items {
            split_item(item, &mut secrets);
        }
    }
    for environment in &mut clean.environments {
        environment.variables = split_variables(&environment.variables, &environment.id, &mut secrets);
    }
    clean.globals = split_variables(&clean.globals, GLOBALS_SCOPE, &mut secrets);
    clean.history = clean.history.iter().map(sanitize_history).collect();

    (clean, secrets)
}

/// Puts vault values back. A value already in the file wins, so it moves to the vault on the next save.
pub fn merge(clean: &Workspace, secrets: &BTreeMap<String, SecretValue>) -> Workspace {
    let mut merged = clean.clone();

    for collection in &mut merged.collections {
        collection.variables = merge_variables(&collection.variables, &collection.id, secrets);
        for item in &mut collection.items {
            merge_item(item, secrets);
        }
    }
    for environment in &mut merged.environments {
        environment.variables = merge_variables(&environment.variables, &environment.id, secrets);
    }
    merged.globals = merge_variables(&merged.globals, GLOBALS_SCOPE, secrets);
    merged
}

/// True when a file written before Secrets existed still holds values that should move.
pub fn needs_migration(loaded: &Workspace) -> bool {
    let (_, secrets) = split(loaded);
    !secrets.is_empty() || loaded.history.iter().any(|entry| sanitize_request(&entry.request) != entry.request)
}

/// History keeps placeholders but no credentials.
pub fn sanitize_history(entry: &HistoryEntry) -> HistoryEntry {
    let mut clean = entry.clone();
    clean.request = sanitize_request(&entry.request);
    clean
}

/// True when a request was saved to History with a credential hidden.
pub fn has_hidden_values(request: &ApiRequest) -> bool {
    request.headers.iter().any(|header| header.enabled && header.value == HIDDEN_VALUE)
}

/// Every field of an auth that is a Secret (ADR 0004). Keep in step with `split_auth` and `merge_auth`.
pub fn auth_secret_values(auth: &RequestAuth) -> Vec<&str> {
    let mut values = vec![auth.token.as_str(), auth.password.as_str(), auth.key_value.as_str()];
    if let Some(details) = &auth.details {
        values.extend([
            details.consumer_secret.as_str(),
            details.token_secret.as_str(),
            details.aws_secret_access_key.as_str(),
            details.aws_session_token.as_str(),
        ]);
    }
    values
}

fn variable_key(scope: &str, name: &str) -> String {
    format!("variable/{scope}/{name}")
}

fn split_variables(variables: &[KeyValue], scope: &str, secrets: &mut BTreeMap<String, SecretValue>) -> Vec<KeyValue> {
    variables
        .iter()
        .map(|variable| {
            if !variable.is_secret || variable.value.is_empty() {
                return variable.clone();
            }
            secrets.insert(variable_key(scope, &variable.key), SecretValue::new(variable.value.clone()));
            KeyValue { value: String::new(), ..variable.clone() }
        })
        .collect()
}

fn merge_variables(variables: &[KeyValue], scope: &str, secrets: &BTreeMap<String, SecretValue>) -> Vec<KeyValue> {
    variables
        .iter()
        .map(|variable| {
            if variable.is_secret && variable.value.is_empty() {
                if let Some(value) = secrets.get(&variable_key(scope, &variable.key)) {
                    return KeyValue { value: value.expose().to_string(), ..variable.clone() };
                }
            }
            variable.clone()
        })
        .collect()
}

fn split_item(item: &mut CollectionItem, secrets: &mut BTreeMap<String, SecretValue>) {
    if let Some(request) = item.request.as_mut() {
        let request_id = request.id.clone();
        if let Some(auth) = request.auth.as_mut() {
            split_auth(auth, &request_id, secrets);
        }
    }
    item.variables = split_variables(&item.variables, &item.id, secrets);
    for child in &mut item.children {
        split_item(child, secrets);
    }
}

fn merge_item(item: &mut CollectionItem, secrets: &BTreeMap<String, SecretValue>) {
    if let Some(request) = item.request.as_mut() {
        let request_id = request.id.clone();
        if let Some(auth) = request.auth.as_mut() {
            merge_auth(auth, &request_id, secrets);
        }
    }
    item.variables = merge_variables(&item.variables, &item.id, secrets);
    for child in &mut item.children {
        merge_item(child, secrets);
    }
}

fn split_auth(auth: &mut RequestAuth, request_id: &str, secrets: &mut BTreeMap<String, SecretValue>) {
    // Placeholders hold no secret, and are safe to share, so only literal values move.
    let mut take = |value: &mut String, field: &str| {
        if is_literal(value) {
            secrets.insert(auth_key(request_id, field), SecretValue::new(value.clone()));
            value.clear();
        }
    };
    take(&mut auth.token, "token");
    take(&mut auth.password, "password");
    take(&mut auth.key_value, "keyValue");
    if let Some(details) = auth.details.as_mut() {
        take(&mut details.consumer_secret, "consumerSecret");
        take(&mut details.token_secret, "tokenSecret");
        take(&mut details.aws_secret_access_key, "awsSecretAccessKey");
        take(&mut details.aws_session_token, "awsSessionToken");
    }
}

fn merge_auth(auth: &mut RequestAuth, request_id: &str, secrets: &BTreeMap<String, SecretValue>) {
    let restore = |value: &mut String, field: &str| {
        if value.is_empty() {
            if let Some(stored) = secrets.get(&auth_key(request_id, field)) {
                *value = stored.expose().to_string();
            }
        }
    };
    restore(&mut auth.token, "token");
    restore(&mut auth.password, "password");
    restore(&mut auth.key_value, "keyValue");
    if let Some(details) = auth.details.as_mut() {
        restore(&mut details.consumer_secret, "consumerSecret");
        restore(&mut details.token_secret, "tokenSecret");
        restore(&mut details.aws_secret_access_key, "awsSecretAccessKey");
        restore(&mut details.aws_session_token, "awsSessionToken");
    }
}

fn is_header_to_hide(header: &KeyValue) -> bool {
    is_sensitive_header(&header.key) && is_literal(&header.value) && header.value != HIDDEN_VALUE
}

fn blank_if_literal(value: &mut String) {
    if is_literal(value) {
        value.clear();
    }
}

fn sanitize_request(request: &ApiRequest) -> ApiRequest {
    let has_hidden_header = request.headers.iter().any(is_header_to_hide);
    let has_literal_auth = request.auth.as_ref().is_some_and(|auth| auth_secret_values(auth).into_iter().any(is_literal));
    if !has_hidden_header && !has_literal_auth {
        return request.clone();
    }

    let mut clean = request.clone();
    for header in &mut clean.headers {
        if is_header_to_hide(header) {
            header.value = HIDDEN_VALUE.to_string();
        }
    }
    if let Some(auth) = clean.auth.as_mut() {
        blank_if_literal(&mut auth.token);
        blank_if_literal(&mut auth.password);
        blank_if_literal(&mut auth.key_value);
        if let Some(details) = auth.details.as_mut() {
            blank_if_literal(&mut details.consumer_secret);
            blank_if_literal(&mut details.token_secret);
            blank_if_literal(&mut details.aws_secret_access_key);
            blank_if_literal(&mut details.aws_session_token);
        }
    }
    clean
}
