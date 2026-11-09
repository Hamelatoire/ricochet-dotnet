//! Port of `OpenApiExporter.cs` and the parts of `CollectionExport.cs` it uses (ADR 0029).
//! Writes a Collection's Requests as an OpenAPI 3.0.3 JSON document: one operation per Request, grouped by Folder.
//! No secret value is written. Sign-in is described by its scheme only, and sensitive headers are left out.
//! Not ported yet: the OpenAPI importer that reads the file back, and the Postman exporter. The export types
//! (`ExportNote`, `ExportResult`) are defined here for now and should move to a shared module when the Postman
//! exporter lands. `SecretRules` is also re-implemented in private helpers below, for the same reason.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::sync::OnceLock;

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::models::{auth_kinds, ApiRequest, Collection, CollectionItem, KeyValue};

/// Categories shown in an export report, as the C# `ExportCategories` names them.
pub mod categories {
    pub const SECRET_VARIABLES: &str = "Secret Variables left out";
    pub const SECRET_VALUES: &str = "Secret values kept out of the export";
    pub const NOT_EXPORTED: &str = "Not exported";
}

const VERSION: &str = "3.0.3";
const DEFAULT_SERVER: &str = "http://localhost";
const METHODS: [&str; 8] = ["get", "put", "post", "delete", "options", "head", "patch", "trace"];

/// Something an export left out or replaced. Shown to the user after every export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportNote {
    pub category: &'static str,
    pub location: String,
    pub detail: String,
}

/// The text of an export, and a note for everything that was left out or replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportResult {
    pub json: String,
    pub notes: Vec<ExportNote>,
}

impl ExportResult {
    /// The notes grouped by category, in the order each category first appears, as the C# `ToReportText` writes them.
    pub fn report_text(&self) -> String {
        let mut groups: Vec<(&'static str, Vec<&ExportNote>)> = Vec::new();
        for note in &self.notes {
            match groups.iter_mut().find(|(category, _)| *category == note.category) {
                Some((_, notes)) => notes.push(note),
                None => groups.push((note.category, vec![note])),
            }
        }

        let mut text = String::new();
        for (category, notes) in groups {
            let _ = writeln!(text, "{category} ({}):", notes.len());
            for note in notes {
                if note.detail.is_empty() {
                    let _ = writeln!(text, "  {}", note.location);
                } else {
                    let _ = writeln!(text, "  {}: {}", note.location, note.detail);
                }
            }
            text.push('\n');
        }

        text.trim_end().to_string()
    }
}

/// Exports a Collection as an OpenAPI 3.0.3 JSON document, with a note for everything left out (ADR 0029).
pub fn export(collection: &Collection) -> ExportResult {
    let mut context = Context::new(collection);
    let mut pending = Vec::new();
    walk(&collection.items, &collection.name, None, &mut context, &mut pending);

    let non_secret = collection.variables.iter().filter(|v| !v.is_secret).count();
    if non_secret > 0 {
        context.notes.push(note(
            categories::NOT_EXPORTED,
            &collection.name,
            format!("{} (OpenAPI has no variables, so their values are not written)", plural(non_secret, "variable")),
        ));
    }

    let servers = servers(&pending, &collection.name, &mut context.notes);
    let mut paths = Map::new();
    let mut schemes = Map::new();
    let mut operation_ids = HashSet::new();
    let mut seen = HashSet::new();

    for item in &pending {
        let method = item.prepared.method.to_lowercase();
        if !METHODS.contains(&method.as_str()) {
            context.notes.push(note(
                categories::NOT_EXPORTED,
                &item.location,
                format!("the method {} has no OpenAPI equivalent", item.prepared.method),
            ));
            continue;
        }

        if !seen.insert(format!("{method} {}", item.url.path)) {
            context.notes.push(note(
                categories::NOT_EXPORTED,
                &item.location,
                "the same method and path as an earlier request, which OpenAPI cannot hold twice",
            ));
            continue;
        }

        let operation = operation(item, &mut operation_ids, &mut schemes);
        let entry = paths.entry(item.url.path.clone()).or_insert_with(|| Value::Object(Map::new()));
        if let Some(operations) = entry.as_object_mut() {
            operations.insert(method, operation);
        }
    }

    let mut document = Map::new();
    document.insert("openapi".into(), json!(VERSION));
    document.insert("info".into(), json!({ "title": collection.name, "version": "1.0.0" }));
    document.insert("servers".into(), Value::Array(servers));
    document.insert("paths".into(), Value::Object(paths));
    if !schemes.is_empty() {
        document.insert("components".into(), json!({ "securitySchemes": schemes }));
    }

    ExportResult {
        json: serde_json::to_string_pretty(&Value::Object(document)).expect("a JSON object always serializes"),
        notes: context.notes,
    }
}

// ----- Shared export context (CollectionExport.cs) -----

/// Holds the Collection's secret values, so every text that goes into the export can be scrubbed of them.
struct Context {
    notes: Vec<ExportNote>,
    /// (name, value), longest value first so a value that contains another is replaced whole.
    secrets: Vec<(String, String)>,
}

impl Context {
    fn new(collection: &Collection) -> Self {
        let mut notes = Vec::new();
        let mut secrets = Vec::new();
        for variable in collection.variables.iter().filter(|v| v.is_secret) {
            let empty = if variable.value.is_empty() { " (has no value on this machine)" } else { "" };
            notes.push(note(categories::SECRET_VARIABLES, &collection.name, format!("{}{empty}", variable.key)));
            if !variable.value.is_empty() {
                secrets.push((variable.key.clone(), variable.value.clone()));
            }
        }
        secrets.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

        Self { notes, secrets }
    }

    /// Returns the text with every known secret value replaced by its placeholder.
    fn scrub(&mut self, text: &str, location: &str) -> String {
        let mut text = text.to_string();
        for (name, value) in &self.secrets {
            if !text.contains(value.as_str()) {
                continue;
            }

            text = text.replace(value.as_str(), &placeholder(name));
            self.notes.push(note(
                categories::SECRET_VALUES,
                location,
                format!("the value of secret {name} was replaced with {}", placeholder(name)),
            ));
        }
        text
    }
}

/// A Request with its secret values already replaced. Only the parts OpenAPI uses are kept.
struct Prepared {
    method: String,
    url: String,
    headers: Vec<KeyValue>,
    body: String,
    auth: PreparedAuth,
}

/// The auth details OpenAPI needs: the kind, and the API key's name and location. Secret values are dropped.
#[derive(Default)]
struct PreparedAuth {
    kind: String,
    key_name: String,
    key_in_query: bool,
}

/// Prepares a Request. Returns the fields whose literal value was typed in and so left out, as the C# `TypedSecrets` does.
fn prepare(context: &mut Context, request: &ApiRequest, location: &str) -> (Prepared, Vec<String>) {
    let mut typed = Vec::new();

    let mut headers = Vec::new();
    for header in request.headers.iter().filter(|h| !h.key.trim().is_empty()) {
        let name = header.key.trim();
        let value = if is_sensitive_header(name) && is_literal(&header.value) {
            typed.push(format!("header {name}"));
            placeholder(&clean(name, "header"))
        } else {
            context.scrub(&header.value, location)
        };
        headers.push(KeyValue { key: name.to_string(), value, enabled: header.enabled, ..KeyValue::default() });
    }

    let source = request.auth.clone().unwrap_or_default();
    let auth = match source.kind.as_str() {
        auth_kinds::BEARER => {
            if is_literal(&source.token) {
                typed.push("bearer token".to_string());
            } else {
                context.scrub(&source.token, location);
            }
            PreparedAuth { kind: auth_kinds::BEARER.to_string(), ..PreparedAuth::default() }
        }
        auth_kinds::BASIC => {
            context.scrub(&source.username, location);
            if is_literal(&source.password) {
                typed.push("basic password".to_string());
            } else {
                context.scrub(&source.password, location);
            }
            PreparedAuth { kind: auth_kinds::BASIC.to_string(), ..PreparedAuth::default() }
        }
        auth_kinds::API_KEY => {
            let key_name = context.scrub(&source.key_name, location);
            if is_literal(&source.key_value) {
                typed.push(format!("API key {}", source.key_name));
            } else {
                context.scrub(&source.key_value, location);
            }
            PreparedAuth { kind: auth_kinds::API_KEY.to_string(), key_name, key_in_query: source.key_in_query }
        }
        // Other kinds are not described, as in the C# `new RequestAuth()`.
        _ => PreparedAuth::default(),
    };

    let url = context.scrub(&request.url, location);
    let body = context.scrub(&request.body, &format!("{location} body"));

    let prepared = Prepared { method: request.method.to_uppercase(), url, headers, body, auth };
    (prepared, typed)
}

// ----- Walking the Collection -----

struct Pending {
    name: String,
    /// The Folder directly holding the Request, or empty for a Request at the top.
    tag: String,
    location: String,
    prepared: Prepared,
    url: ParsedUrl,
}

fn walk(items: &[CollectionItem], location: &str, tag: Option<&str>, context: &mut Context, pending: &mut Vec<Pending>) {
    for item in items {
        let path = format!("{location} / {}", item.name);
        if !item.scripts.is_empty() {
            context.notes.push(note(
                categories::NOT_EXPORTED,
                &path,
                format!("{} (OpenAPI has no place for scripts)", plural(item.scripts.len(), "script")),
            ));
        }

        match &item.request {
            Some(request) => {
                if let Some(assertions) = request.assertions.as_ref().filter(|a| !a.is_empty()) {
                    context.notes.push(note(
                        categories::NOT_EXPORTED,
                        &path,
                        format!("{} (OpenAPI has no tests)", plural(assertions.len(), "assertion")),
                    ));
                }

                let (prepared, typed) = prepare(context, request, &path);
                for field in typed {
                    context.notes.push(note(
                        categories::SECRET_VALUES,
                        &path,
                        format!("{field} was typed in as a value, so it is not written. The sign-in scheme still says what is needed."),
                    ));
                }

                let url = parse_url(&prepared.url);
                pending.push(Pending { name: item.name.clone(), tag: tag.unwrap_or("").to_string(), location: path, prepared, url });
            }
            None => walk(&item.children, &path, Some(&item.name), context, pending),
        }
    }
}

fn servers(pending: &[Pending], collection_name: &str, notes: &mut Vec<ExportNote>) -> Vec<Value> {
    let mut origins: Vec<&str> = Vec::new();
    let mut placeholders: Vec<&str> = Vec::new();
    for origin in pending.iter().filter_map(|p| p.url.origin.as_deref()) {
        let list = if origin.contains("{{") { &mut placeholders } else { &mut origins };
        if !list.contains(&origin) {
            list.push(origin);
        }
    }

    let mut servers: Vec<Value> = origins.iter().map(|origin| json!({ "url": origin })).collect();

    if !placeholders.is_empty() {
        notes.push(note(
            categories::NOT_EXPORTED,
            collection_name,
            format!("requests start with {}, which is not a server address, so set the server by hand", placeholders.join(", ")),
        ));
    }

    if servers.is_empty() {
        servers.push(json!({ "url": DEFAULT_SERVER, "description": "Replace with the real server address." }));
        notes.push(note(
            categories::NOT_EXPORTED,
            collection_name,
            format!("no request has a full address, so the server is set to {DEFAULT_SERVER}"),
        ));
    }

    servers
}

fn operation(item: &Pending, operation_ids: &mut HashSet<String>, schemes: &mut Map<String, Value>) -> Value {
    let prepared = &item.prepared;
    let mut node = Map::new();
    node.insert("operationId".into(), json!(unique_id(&item.name, operation_ids)));
    node.insert("summary".into(), json!(item.name));
    if !item.tag.is_empty() {
        node.insert("tags".into(), json!([item.tag]));
    }

    let mut parameters = Vec::new();
    let mut added = HashSet::new();
    for name in &item.url.path_params {
        if added.insert(format!("path {name}")) {
            parameters.push(parameter(name, "path", true));
        }
    }
    for name in &item.url.query_keys {
        if added.insert(format!("query {name}")) {
            parameters.push(parameter(name, "query", false));
        }
    }
    for header in &prepared.headers {
        // Sensitive headers stay out entirely: their scheme is described below, and their values never appear.
        if !header.enabled || is_sensitive_header(&header.key) || header.key.eq_ignore_ascii_case("Content-Type") {
            continue;
        }
        if added.insert(format!("header {}", header.key)) {
            parameters.push(parameter(&header.key, "header", false));
        }
    }
    if !parameters.is_empty() {
        node.insert("parameters".into(), Value::Array(parameters));
    }

    if !prepared.body.is_empty() {
        let media_type = content_type(&prepared.headers)
            .map(|value| value.split(';').next().unwrap_or("").trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| {
                let trimmed = prepared.body.trim_start();
                if trimmed.starts_with('{') || trimmed.starts_with('[') { "application/json" } else { "text/plain" }.to_string()
            });

        let mut media = Map::new();
        media.insert("schema".into(), json!({ "type": "string" }));
        media.insert("example".into(), json!(prepared.body));
        let mut content = Map::new();
        content.insert(media_type, Value::Object(media));
        node.insert("requestBody".into(), json!({ "content": content }));
    }

    if let Some(scheme) = security_scheme(&prepared.auth, schemes) {
        let mut requirement = Map::new();
        requirement.insert(scheme, Value::Array(Vec::new()));
        node.insert("security".into(), Value::Array(vec![Value::Object(requirement)]));
    }

    node.insert("responses".into(), json!({ "default": { "description": "Response" } }));
    Value::Object(node)
}

fn parameter(name: &str, location: &str, required: bool) -> Value {
    json!({ "name": name, "in": location, "required": required, "schema": { "type": "string" } })
}

/// The security scheme for a Request's auth, added to `schemes` the first time it is used. None when the auth sends no scheme.
fn security_scheme(auth: &PreparedAuth, schemes: &mut Map<String, Value>) -> Option<String> {
    let (name, scheme) = match auth.kind.as_str() {
        auth_kinds::BEARER => ("bearerAuth".to_string(), json!({ "type": "http", "scheme": "bearer" })),
        auth_kinds::BASIC => ("basicAuth".to_string(), json!({ "type": "http", "scheme": "basic" })),
        auth_kinds::API_KEY if !auth.key_name.is_empty() => {
            let location = if auth.key_in_query { "query" } else { "header" };
            (
                format!("apiKey_{}_{location}", clean(&auth.key_name, "custom")),
                json!({ "type": "apiKey", "name": auth.key_name, "in": location }),
            )
        }
        _ => return None,
    };

    if !schemes.contains_key(&name) {
        schemes.insert(name.clone(), scheme);
    }
    Some(name)
}

/// An operationId built from the Request's name, letters and digits only, made unique with a number.
fn unique_id(name: &str, taken: &mut HashSet<String>) -> String {
    let mut base = String::new();
    for c in name.chars().filter(|c| c.is_ascii_alphanumeric()) {
        if base.is_empty() {
            base.push(c.to_ascii_lowercase());
        } else {
            base.push(c);
        }
    }
    if base.is_empty() {
        base = "request".to_string();
    }

    let mut id = base.clone();
    let mut n = 2;
    while !taken.insert(id.clone()) {
        id = format!("{base}{n}");
        n += 1;
    }
    id
}

// ----- URLs -----

struct ParsedUrl {
    /// The scheme and host, or a `{{placeholder}}` standing for them. None when the URL has neither.
    origin: Option<String>,
    path: String,
    path_params: Vec<String>,
    query_keys: Vec<String>,
}

fn parse_url(url: &str) -> ParsedUrl {
    let mut url = url;
    if let Some(i) = url.find('#') {
        url = &url[..i];
    }

    let mut query = "";
    if let Some(i) = url.find('?') {
        query = &url[i + 1..];
        url = &url[..i];
    }

    let mut origin = None;
    let mut rest = url;
    if url.starts_with("{{") {
        if let Some(close) = url.find("}}") {
            origin = Some(url[..close + 2].to_string());
            rest = &url[close + 2..];
        }
    } else if let Some(scheme) = url.find("://") {
        match url[scheme + 3..].find('/').map(|i| i + scheme + 3) {
            Some(slash) => {
                origin = Some(url[..slash].to_string());
                rest = &url[slash..];
            }
            None => {
                origin = Some(url.to_string());
                rest = "";
            }
        }
    }

    let mut path_params: Vec<String> = Vec::new();
    let replaced = placeholder_pattern().replace_all(rest, |caps: &regex::Captures| {
        let name = clean(&caps[1], "param");
        if !path_params.contains(&name) {
            path_params.push(name.clone());
        }
        format!("{{{name}}}")
    });

    let mut path = replaced.into_owned();
    if path.is_empty() {
        path = "/".to_string();
    } else if !path.starts_with('/') {
        path.insert(0, '/');
    }

    let mut query_keys: Vec<String> = Vec::new();
    for part in query.split('&').filter(|p| !p.is_empty()) {
        let key = part.split('=').next().unwrap_or("");
        if !key.is_empty() && !key.contains("{{") && !query_keys.iter().any(|k| k == key) {
            query_keys.push(key.to_string());
        }
    }

    ParsedUrl { origin, path, path_params, query_keys }
}

// ----- Small helpers (SecretRules.cs, PreparedRequest) -----

fn placeholder_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"\{\{([^{}]+)\}\}").expect("placeholder pattern is valid"))
}

fn placeholder(name: &str) -> String {
    format!("{{{{{name}}}}}")
}

/// True when the text holds an actual value, not just `{{placeholders}}`, which are safe to share.
fn is_literal(value: &str) -> bool {
    !placeholder_pattern().replace_all(value, "").trim().is_empty()
}

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

fn is_sensitive_header(name: &str) -> bool {
    let name = name.trim();
    SENSITIVE_HEADERS.iter().any(|h| h.eq_ignore_ascii_case(name))
}

/// The first enabled Content-Type header's value.
fn content_type(headers: &[KeyValue]) -> Option<&str> {
    headers
        .iter()
        .find(|h| h.enabled && h.key.eq_ignore_ascii_case("Content-Type"))
        .map(|h| h.value.as_str())
}

/// A name that can stand inside `{{ }}` and as a parameter name, or the fallback.
fn clean(name: &str, fallback: &str) -> String {
    if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')) {
        name.to_string()
    } else {
        fallback.to_string()
    }
}

fn plural(count: usize, noun: &str) -> String {
    if count == 1 { format!("1 {noun}") } else { format!("{count} {noun}s") }
}

fn note(category: &'static str, location: &str, detail: impl Into<String>) -> ExportNote {
    ExportNote { category, location: location.to_string(), detail: detail.into() }
}
