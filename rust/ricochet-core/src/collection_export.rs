//! Port of the shared parts of `CollectionExport.cs` (ADR 0002, ADR 0004, ADR 0029): the report of what an
//! export left out or replaced, and the Secret rules every exporter follows. The Postman and OpenAPI exporters
//! build on this module and are ported separately; the file format itself is not written here.
//!
//! `SecretRules` (`WorkspaceSecrets.cs`) is not ported yet, so `is_literal` and `is_sensitive_header` below
//! copy its two rules. Switch to the shared ones when that port lands.

use std::sync::OnceLock;

use regex::Regex;

use crate::models::{ApiRequest, Collection, KeyValue, RequestAuth, auth_kinds};

/// Category names shown in the export report, as in `ExportCategories`.
pub mod categories {
    pub const SECRET_VARIABLES: &str = "Secret Variables left out";
    pub const SECRET_VALUES: &str = "Secret values kept out of the export";
    pub const NOT_EXPORTED: &str = "Not exported";
}

/// Something an export left out or replaced. Shown to the person after every export, as imports are (ADR 0002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportNote {
    pub category: String,
    pub location: String,
    pub detail: String,
}

impl ExportNote {
    pub fn new(category: &str, location: &str, detail: impl Into<String>) -> Self {
        Self { category: category.to_string(), location: location.to_string(), detail: detail.into() }
    }
}

/// The text of an export, and a note for everything that was left out or replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportResult {
    pub json: String,
    pub notes: Vec<ExportNote>,
}

impl ExportResult {
    /// Notes grouped by category, in the order each category first appears. Trailing blank lines are removed.
    pub fn to_report_text(&self) -> String {
        let mut categories_in_order: Vec<&str> = Vec::new();
        for note in &self.notes {
            if !categories_in_order.contains(&note.category.as_str()) {
                categories_in_order.push(&note.category);
            }
        }

        let mut text = String::new();
        for category in categories_in_order {
            let group: Vec<&ExportNote> = self.notes.iter().filter(|n| n.category == category).collect();
            text.push_str(&format!("{category} ({}):\n", group.len()));
            for note in group {
                if note.detail.is_empty() {
                    text.push_str(&format!("  {}\n", note.location));
                } else {
                    text.push_str(&format!("  {}: {}\n", note.location, note.detail));
                }
            }
            text.push('\n');
        }

        text.trim_end().to_string()
    }
}

/// Shared by the exporters. Secret values (ADR 0004) never reach an export: Secret Variables are left out,
/// and any other text that contains their value has it replaced with the Variable's placeholder.
#[derive(Debug, Clone, Default)]
pub struct ExportContext {
    secret_values: Vec<(String, String)>,
    pub notes: Vec<ExportNote>,
}

impl ExportContext {
    pub fn new(collection: &Collection) -> Self {
        let mut context = Self::default();
        for variable in collection.variables.iter().filter(|v| v.is_secret) {
            let empty = if variable.value.is_empty() { " (has no value on this machine)" } else { "" };
            context.notes.push(ExportNote::new(
                categories::SECRET_VARIABLES,
                &collection.name,
                format!("{}{empty}", variable.key),
            ));
            if !variable.value.is_empty() {
                context.secret_values.push((variable.key.clone(), variable.value.clone()));
            }
        }

        // Longest first, so a value that contains another is replaced whole.
        context.secret_values.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
        context
    }

    /// Returns the text with every known Secret value replaced by its placeholder, and notes each one replaced.
    pub fn scrub(&mut self, text: &str, location: &str) -> String {
        let mut text = text.to_string();
        for (name, value) in &self.secret_values {
            if !text.contains(value.as_str()) {
                continue;
            }

            let placeholder = placeholder(name);
            text = text.replace(value.as_str(), &placeholder);
            self.notes.push(ExportNote::new(
                categories::SECRET_VALUES,
                location,
                format!("the value of secret {name} was replaced with {placeholder}"),
            ));
        }

        text
    }
}

/// A value typed directly into a Request, such as a bearer token (ADR 0004), and the placeholder written instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedSecret {
    pub field: String,
    pub placeholder: String,
}

/// A Request with its Secret values already replaced, ready for either export format.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<KeyValue>,
    pub body: String,
    pub auth: RequestAuth,
    pub typed_secrets: Vec<TypedSecret>,
}

impl PreparedRequest {
    pub fn create(context: &mut ExportContext, request: &ApiRequest, location: &str) -> Self {
        let mut typed = Vec::new();

        let mut headers = Vec::new();
        for header in request.headers.iter().filter(|h| !h.key.trim().is_empty()) {
            let name = header.key.trim();
            let value = if is_sensitive_header(name) && is_literal(&header.value) {
                literal(&mut typed, &clean(name, "header"), &format!("header {name}"))
            } else {
                context.scrub(&header.value, location)
            };
            headers.push(KeyValue { key: name.to_string(), value, enabled: header.enabled, ..KeyValue::default() });
        }

        let source = request.auth.clone().unwrap_or_default();
        let auth = match source.kind.as_str() {
            auth_kinds::BEARER => RequestAuth {
                kind: auth_kinds::BEARER.to_string(),
                token: if is_literal(&source.token) {
                    literal(&mut typed, "token", "bearer token")
                } else {
                    context.scrub(&source.token, location)
                },
                ..RequestAuth::default()
            },
            auth_kinds::BASIC => {
                let username = context.scrub(&source.username, location);
                let password = if is_literal(&source.password) {
                    literal(&mut typed, "password", "basic password")
                } else {
                    context.scrub(&source.password, location)
                };
                RequestAuth {
                    kind: auth_kinds::BASIC.to_string(),
                    username,
                    password,
                    ..RequestAuth::default()
                }
            }
            auth_kinds::API_KEY => {
                let key_name = context.scrub(&source.key_name, location);
                let key_value = if is_literal(&source.key_value) {
                    literal(
                        &mut typed,
                        &clean(&source.key_name, "apiKey"),
                        &format!("API key {}", source.key_name),
                    )
                } else {
                    context.scrub(&source.key_value, location)
                };
                RequestAuth {
                    kind: auth_kinds::API_KEY.to_string(),
                    key_name,
                    key_value,
                    key_in_query: source.key_in_query,
                    ..RequestAuth::default()
                }
            }
            // Other kinds are not written, so their fields (which may hold Secrets) are dropped.
            _ => RequestAuth::default(),
        };

        let url = context.scrub(&request.url, location);
        let body = context.scrub(&request.body, &format!("{location} body"));

        Self {
            method: request.method.to_uppercase(),
            url,
            headers,
            body,
            auth,
            typed_secrets: typed,
        }
    }
}

/// The `{{name}}` form a Variable's placeholder takes in an export.
pub fn placeholder(name: &str) -> String {
    format!("{{{{{name}}}}}")
}

/// `1 folder` or `3 folders`.
pub fn plural(count: usize, noun: &str) -> String {
    if count == 1 { format!("1 {noun}") } else { format!("{count} {noun}s") }
}

/// The value of the first enabled Content-Type header, compared without regard to case.
pub fn content_type(headers: &[KeyValue]) -> Option<&str> {
    headers
        .iter()
        .find(|h| h.enabled && h.key.eq_ignore_ascii_case("Content-Type"))
        .map(|h| h.value.as_str())
}

/// A name that can stand inside `{{ }}` and as a parameter name, or the fallback.
pub fn clean(name: &str, fallback: &str) -> String {
    if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')) {
        name.to_string()
    } else {
        fallback.to_string()
    }
}

/// True when the text holds an actual value, not just `{{placeholders}}` that are safe to share.
/// Copy of `SecretRules.IsLiteral` until that is ported.
pub fn is_literal(value: &str) -> bool {
    static PLACEHOLDER: OnceLock<Regex> = OnceLock::new();
    let pattern = PLACEHOLDER.get_or_init(|| Regex::new(r"\{\{[^{}]+\}\}").expect("valid pattern"));
    !pattern.replace_all(value, "").trim().is_empty()
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

/// Copy of `SecretRules.IsSensitiveHeader` until that is ported.
pub fn is_sensitive_header(name: &str) -> bool {
    let name = name.trim();
    SENSITIVE_HEADERS.iter().any(|header| header.eq_ignore_ascii_case(name))
}

/// Records a typed value and returns the placeholder written in its place.
fn literal(typed: &mut Vec<TypedSecret>, placeholder_name: &str, field: &str) -> String {
    let placeholder = placeholder(placeholder_name);
    typed.push(TypedSecret { field: field.to_string(), placeholder: placeholder.clone() });
    placeholder
}
