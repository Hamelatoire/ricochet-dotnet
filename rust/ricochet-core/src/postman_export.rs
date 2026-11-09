//! Port of `PostmanExporter.cs` and the parts of `CollectionExport.cs` it uses (ADR 0029).
//!
//! Writes a Collection as a Postman v2.1 collection. Secret values become `{{placeholders}}` and Secret
//! Variables are left out, so no Secret value reaches the output. Output has the same shape as the C# export.
//! Object keys come out sorted, because `serde_json` keeps maps sorted here; C# keeps insertion order. Postman
//! does not depend on key order.
//!
//! The shared export types (`ExportNote`, `ExportResult`, `ExportContext`) live here until the OpenAPI
//! exporter is ported. Not ported: the Postman importer, and the OpenAPI exporter.

use std::fmt::Write as _;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::models::{auth_kinds, ApiRequest, Collection, CollectionItem, KeyValue, RequestAuth, StoredScript};

/// The schema URL written into `info.schema`.
pub const SCHEMA: &str = "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";

/// Categories of `ExportNote`, as the C# `ExportCategories` names them.
pub mod categories {
    pub const SECRET_VARIABLES: &str = "Secret Variables left out";
    pub const SECRET_VALUES: &str = "Secret values kept out of the export";
    pub const NOT_EXPORTED: &str = "Not exported";
}

/// Something an export left out or replaced. Shown to the user after every export, as imports are (ADR 0002).
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
    /// Notes grouped by category, in the order each category first appears. Ends without a trailing newline.
    pub fn to_report_text(&self) -> String {
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

/// Exports a Collection as a Postman v2.1 collection (ADR 0029).
pub fn export(collection: &Collection) -> ExportResult {
    let mut context = ExportContext::new(collection);
    let mut root = Map::new();

    let mut info = Map::new();
    info.insert("_postman_id".into(), json!(collection.id));
    info.insert("name".into(), json!(collection.name));
    info.insert("schema".into(), json!(SCHEMA));
    root.insert("info".into(), Value::Object(info));
    root.insert("item".into(), Value::Array(export_items(&collection.items, &mut context, &collection.name)));

    let mut variables = Vec::new();
    for variable in collection.variables.iter().filter(|v| !v.is_secret) {
        let location = format!("{} variable {}", collection.name, variable.key);
        let mut entry = Map::new();
        entry.insert("key".into(), json!(variable.key));
        entry.insert("value".into(), json!(context.scrub(&variable.value, &location)));
        if !variable.enabled {
            entry.insert("disabled".into(), json!(true));
        }
        variables.push(Value::Object(entry));
    }
    if !variables.is_empty() {
        root.insert("variable".into(), Value::Array(variables));
    }

    let events = export_events(&collection.scripts, &mut context, &collection.name);
    if !events.is_empty() {
        root.insert("event".into(), Value::Array(events));
    }

    ExportResult {
        json: serde_json::to_string_pretty(&Value::Object(root)).expect("a JSON value always serializes"),
        notes: context.notes,
    }
}

fn export_items(items: &[CollectionItem], context: &mut ExportContext, location: &str) -> Vec<Value> {
    let mut array = Vec::new();
    for item in items {
        let path = format!("{location} / {}", item.name);
        let mut node = Map::new();
        node.insert("name".into(), json!(item.name));
        match &item.request {
            Some(request) => {
                node.insert("request".into(), Value::Object(request_node(request, context, &path)));
            }
            None => {
                node.insert("item".into(), Value::Array(export_items(&item.children, context, &path)));
            }
        }

        let events = export_events(&item.scripts, context, &path);
        if !events.is_empty() {
            node.insert("event".into(), Value::Array(events));
        }

        array.push(Value::Object(node));
    }

    array
}

fn request_node(request: &ApiRequest, context: &mut ExportContext, location: &str) -> Map<String, Value> {
    let prepared = prepare(context, request, location);
    for typed in &prepared.typed_secrets {
        context.notes.push(ExportNote {
            category: categories::SECRET_VALUES,
            location: location.to_string(),
            detail: format!(
                "{} was typed in as a value, so {} is written instead. Set it as a Variable.",
                typed.field, typed.placeholder
            ),
        });
    }

    if let Some(assertions) = request.assertions.as_ref().filter(|a| !a.is_empty()) {
        context.notes.push(ExportNote {
            category: categories::NOT_EXPORTED,
            location: location.to_string(),
            detail: format!("{} (Postman tests are not written)", plural(assertions.len(), "assertion")),
        });
    }
    if let Some(captures) = request.captures.as_ref().filter(|c| !c.is_empty()) {
        context.notes.push(ExportNote {
            category: categories::NOT_EXPORTED,
            location: location.to_string(),
            detail: plural(captures.len(), "capture"),
        });
    }

    let mut node = Map::new();
    node.insert("method".into(), json!(prepared.method));
    node.insert("header".into(), Value::Array(headers_json(&prepared.headers)));
    node.insert("url".into(), json!({ "raw": prepared.url }));
    node.insert("auth".into(), auth_json(&prepared.auth));

    if !prepared.body.is_empty() {
        node.insert(
            "body".into(),
            json!({
                "mode": "raw",
                "raw": prepared.body,
                "options": { "raw": { "language": language(&prepared.headers) } },
            }),
        );
    }

    node
}

fn headers_json(headers: &[KeyValue]) -> Vec<Value> {
    headers
        .iter()
        .map(|header| {
            let mut entry = Map::new();
            entry.insert("key".into(), json!(header.key));
            entry.insert("value".into(), json!(header.value));
            if !header.enabled {
                entry.insert("disabled".into(), json!(true));
            }
            Value::Object(entry)
        })
        .collect()
}

fn auth_json(auth: &PreparedAuth) -> Value {
    match auth {
        PreparedAuth::Bearer { token } => json!({
            "type": "bearer",
            "bearer": pairs(&[("token", token)]),
        }),
        PreparedAuth::Basic { username, password } => json!({
            "type": "basic",
            "basic": pairs(&[("username", username), ("password", password)]),
        }),
        PreparedAuth::ApiKey { key_name, key_value, in_query } => json!({
            "type": "apikey",
            "apikey": pairs(&[
                ("key", key_name),
                ("value", key_value),
                ("in", if *in_query { "query" } else { "header" }),
            ]),
        }),
        PreparedAuth::None => json!({ "type": "noauth" }),
    }
}

fn pairs(pairs: &[(&str, &str)]) -> Value {
    Value::Array(
        pairs
            .iter()
            .map(|(key, value)| json!({ "key": key, "value": value, "type": "string" }))
            .collect(),
    )
}

fn language(headers: &[KeyValue]) -> &'static str {
    let content_type = content_type(headers).unwrap_or("").to_ascii_lowercase();
    if content_type.contains("json") {
        "json"
    } else if content_type.contains("xml") {
        "xml"
    } else if content_type.contains("html") {
        "html"
    } else if content_type.contains("javascript") {
        "javascript"
    } else {
        "text"
    }
}

fn content_type(headers: &[KeyValue]) -> Option<&str> {
    headers
        .iter()
        .find(|h| h.enabled && h.key.eq_ignore_ascii_case("Content-Type"))
        .map(|h| h.value.as_str())
}

fn export_events(scripts: &[StoredScript], context: &mut ExportContext, location: &str) -> Vec<Value> {
    let mut array = Vec::new();
    for script in scripts {
        let source = context.scrub(&script.source, location);
        let lines: Vec<Value> = source.split('\n').map(|line| json!(line)).collect();
        array.push(json!({
            "listen": script.listen,
            "script": { "type": "text/javascript", "exec": lines },
        }));
    }

    array
}

fn plural(count: usize, noun: &str) -> String {
    if count == 1 { format!("1 {noun}") } else { format!("{count} {noun}s") }
}

/// Shared by the exporters. Secret values (ADR 0004) never reach an export: Secret Variables are left out,
/// and any other text that contains their value has it replaced with the Variable's placeholder.
pub(crate) struct ExportContext {
    /// (name, value), longest value first, so a value that contains another is replaced whole.
    secret_values: Vec<(String, String)>,
    pub(crate) notes: Vec<ExportNote>,
}

impl ExportContext {
    pub(crate) fn new(collection: &Collection) -> Self {
        let mut notes = Vec::new();
        let mut secret_values = Vec::new();
        for variable in collection.variables.iter().filter(|v| v.is_secret) {
            let empty = if variable.value.is_empty() { " (has no value on this machine)" } else { "" };
            notes.push(ExportNote {
                category: categories::SECRET_VARIABLES,
                location: collection.name.clone(),
                detail: format!("{}{empty}", variable.key),
            });
            if !variable.value.is_empty() {
                secret_values.push((variable.key.clone(), variable.value.clone()));
            }
        }

        secret_values.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
        Self { secret_values, notes }
    }

    /// Returns the text with every known secret value replaced by its placeholder.
    pub(crate) fn scrub(&mut self, text: &str, location: &str) -> String {
        let mut text = text.to_string();
        for (name, value) in &self.secret_values {
            if !text.contains(value.as_str()) {
                continue;
            }

            let placeholder = placeholder(name);
            text = text.replace(value.as_str(), &placeholder);
            self.notes.push(ExportNote {
                category: categories::SECRET_VALUES,
                location: location.to_string(),
                detail: format!("the value of secret {name} was replaced with {placeholder}"),
            });
        }

        text
    }
}

/// `{{name}}`, the placeholder a Secret is written as.
fn placeholder(name: &str) -> String {
    let mut text = String::from("{{");
    text.push_str(name);
    text.push_str("}}");
    text
}

/// A value typed directly into a Request, such as a bearer token (ADR 0004), and the placeholder written instead.
struct TypedSecret {
    field: String,
    placeholder: String,
}

/// The auth written for a Request. Only the chosen kind's fields are kept, as in the C# export.
enum PreparedAuth {
    None,
    Bearer { token: String },
    Basic { username: String, password: String },
    ApiKey { key_name: String, key_value: String, in_query: bool },
}

/// A Request with its secret values already replaced, ready to write.
struct Prepared {
    method: String,
    url: String,
    headers: Vec<KeyValue>,
    body: String,
    auth: PreparedAuth,
    typed_secrets: Vec<TypedSecret>,
}

fn prepare(context: &mut ExportContext, request: &ApiRequest, location: &str) -> Prepared {
    let mut typed = Vec::new();

    let mut headers = Vec::new();
    for header in request.headers.iter().filter(|h| !h.key.trim().is_empty()) {
        let name = header.key.trim();
        let value = if is_sensitive_header(name) && is_literal(&header.value) {
            literal(&mut typed, &clean(name, "header"), format!("header {name}"))
        } else {
            context.scrub(&header.value, location)
        };
        headers.push(KeyValue { key: name.to_string(), value, enabled: header.enabled, ..KeyValue::default() });
    }

    let default_auth = RequestAuth::default();
    let source = request.auth.as_ref().unwrap_or(&default_auth);
    let auth = match source.kind.as_str() {
        auth_kinds::BEARER => {
            let token = if is_literal(&source.token) {
                literal(&mut typed, "token", "bearer token".to_string())
            } else {
                context.scrub(&source.token, location)
            };
            PreparedAuth::Bearer { token }
        }
        auth_kinds::BASIC => {
            let username = context.scrub(&source.username, location);
            let password = if is_literal(&source.password) {
                literal(&mut typed, "password", "basic password".to_string())
            } else {
                context.scrub(&source.password, location)
            };
            PreparedAuth::Basic { username, password }
        }
        auth_kinds::API_KEY => {
            let key_name = context.scrub(&source.key_name, location);
            let key_value = if is_literal(&source.key_value) {
                literal(&mut typed, &clean(&source.key_name, "apiKey"), format!("API key {}", source.key_name))
            } else {
                context.scrub(&source.key_value, location)
            };
            PreparedAuth::ApiKey { key_name, key_value, in_query: source.key_in_query }
        }
        _ => PreparedAuth::None,
    };

    let url = context.scrub(&request.url, location);
    let body = context.scrub(&request.body, &format!("{location} body"));

    Prepared {
        method: request.method.to_uppercase(),
        url,
        headers,
        body,
        auth,
        typed_secrets: typed,
    }
}

/// Records a typed-in value and returns the placeholder written in its place.
fn literal(typed: &mut Vec<TypedSecret>, placeholder_name: &str, field: String) -> String {
    let placeholder = placeholder(placeholder_name);
    typed.push(TypedSecret { field, placeholder: placeholder.clone() });
    placeholder
}

static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{\{[^{}]+\}\}").expect("valid regex"));

/// True when the text holds an actual value, not just `{{placeholders}}` that are safe to share.
fn is_literal(value: &str) -> bool {
    !PLACEHOLDER.replace_all(value, "").trim().is_empty()
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
    SENSITIVE_HEADERS.iter().any(|known| known.eq_ignore_ascii_case(name.trim()))
}

/// A name that can stand inside `{{ }}` and as a parameter name, or the fallback.
fn clean(name: &str, fallback: &str) -> String {
    let usable = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if usable { name.to_string() } else { fallback.to_string() }
}
