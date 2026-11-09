//! Port of `CollectionDocumentation.cs` (ADR 0028). Renders one Collection as a single self-contained HTML
//! page for people who do not use Ricochet. The page has no scripts, no external styles and no network calls.
//! Secret values and Secret Variables are written as [`MASKED`], and every piece of text is HTML-escaped.
//! Hidden Folders and Requests (ADR 0032) are left out.
//!
//! Not ported yet: the `ricochet docs` command and its file checks (refuse a file inside the Workspace,
//! refuse an existing file unless forced). The secret rules are copied as private helpers until `SecretRules`
//! is ported, and they must be kept in step with the C# file.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::{Captures, Regex};

use crate::models::{ApiRequest, Collection, CollectionItem, KeyValue, RequestAuth, auth_kinds};

/// What a Secret looks like in the page. It stands in for the value and never reveals its length.
pub const MASKED: &str = "***";

const FIRST_HEADING_LEVEL: usize = 2;
const LAST_HEADING_LEVEL: usize = 6;

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

static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{\{[^{}]+\}\}").expect("valid regex"));

static SENSITIVE_QUERY_PARAMETER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)[?&](api[_-]?key|access[_-]?token|auth[_-]?token|token|password|passwd|secret)=([^&#]*)")
        .expect("valid regex")
});

const CSS: &str = r#":root { color-scheme: light dark; --fg: #1b1f24; --muted: #59636e; --line: #d0d7de; --panel: #f6f8fa; }
@media (prefers-color-scheme: dark) { :root { --fg: #e6edf3; --muted: #9198a1; --line: #3d444d; --panel: #161b22; } }
body { margin: 0; background: transparent; color: var(--fg); font: 15px/1.5 system-ui, -apple-system, "Segoe UI", sans-serif; }
main { max-width: 960px; margin: 0 auto; padding: 24px 16px 64px; }
h1 { font-size: 28px; margin: 0 0 16px; }
h2, h3, h4, h5, h6 { margin: 24px 0 8px; }
section.folder { border-left: 2px solid var(--line); padding-left: 16px; margin-top: 16px; }
article.request { border: 1px solid var(--line); border-radius: 6px; padding: 12px 16px; margin: 12px 0; }
.method { display: inline-block; min-width: 4em; font-weight: 600; font-family: ui-monospace, Consolas, monospace; }
.url { font-family: ui-monospace, Consolas, monospace; word-break: break-all; margin: 4px 0 8px; }
.label { color: var(--muted); font-size: 13px; text-transform: uppercase; letter-spacing: .04em; margin: 12px 0 4px; }
table { border-collapse: collapse; width: 100%; font-size: 14px; }
th, td { text-align: left; padding: 4px 8px; border-bottom: 1px solid var(--line); vertical-align: top; word-break: break-all; }
th { color: var(--muted); font-weight: 600; }
pre { background: var(--panel); border-radius: 6px; padding: 12px; overflow-x: auto; white-space: pre-wrap; word-break: break-word; }
code { font-family: ui-monospace, Consolas, monospace; }
dl { display: grid; grid-template-columns: max-content 1fr; gap: 4px 12px; margin: 4px 0; }
dt { color: var(--muted); }
dd { margin: 0; word-break: break-all; }"#;

/// Returns the whole page as one string. The output depends only on the Collection, so the same input gives
/// the same page.
pub fn render(collection: &Collection) -> String {
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n");
    out.push_str("<html lang=\"en\">\n<head>\n");
    out.push_str("<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str("<meta name=\"generator\" content=\"Ricochet\">\n");
    out.push_str("<title>");
    push_escaped(&mut out, &collection.name);
    out.push_str("</title>\n");
    out.push_str("<style>\n");
    out.push_str(CSS);
    out.push_str("\n</style>\n");
    out.push_str("</head>\n<body>\n<main>\n");
    out.push_str("<h1>");
    push_escaped(&mut out, &collection.name);
    out.push_str("</h1>\n");

    append_variables(&mut out, &collection.variables);

    let hidden: HashSet<&str> = collection
        .hidden_item_ids
        .iter()
        .flatten()
        .map(String::as_str)
        .collect();
    append_items(&mut out, &collection.items, FIRST_HEADING_LEVEL, &hidden);

    out.push_str("</main>\n</body>\n</html>\n");
    out
}

fn append_variables(out: &mut String, variables: &[KeyValue]) {
    let enabled: Vec<&KeyValue> = variables
        .iter()
        .filter(|v| v.enabled && !v.key.is_empty())
        .collect();
    if enabled.is_empty() {
        return;
    }

    out.push_str("<section>\n<h2>Variables</h2>\n");
    let rows: Vec<(&str, String)> = enabled
        .iter()
        .map(|v| (v.key.as_str(), if v.is_secret { MASKED.to_string() } else { v.value.clone() }))
        .collect();
    append_table(out, &["Name", "Value"], &rows);
    out.push_str("</section>\n");
}

fn append_items(out: &mut String, items: &[CollectionItem], level: usize, hidden: &HashSet<&str>) {
    for item in items {
        if hidden.contains(item.id.as_str()) {
            continue;
        }

        match &item.request {
            None => append_folder(out, item, level, hidden),
            Some(request) => append_request(out, item, request, level),
        }
    }
}

fn append_folder(out: &mut String, folder: &CollectionItem, level: usize, hidden: &HashSet<&str>) {
    let heading = heading(level);
    out.push_str("<section class=\"folder\">\n");
    out.push('<');
    out.push_str(&heading);
    out.push('>');
    push_escaped(out, &folder.name);
    out.push_str("</");
    out.push_str(&heading);
    out.push_str(">\n");
    append_items(out, &folder.children, level + 1, hidden);
    out.push_str("</section>\n");
}

fn append_request(out: &mut String, item: &CollectionItem, request: &ApiRequest, level: usize) {
    let heading = heading(level);
    let title = if !request.name.is_empty() {
        request.name.as_str()
    } else if !item.name.is_empty() {
        item.name.as_str()
    } else {
        request.url.as_str()
    };

    out.push_str("<article class=\"request\">\n");
    out.push('<');
    out.push_str(&heading);
    out.push_str("><span class=\"method\">");
    push_escaped(out, &request.method);
    out.push_str("</span> ");
    push_escaped(out, title);
    out.push_str("</");
    out.push_str(&heading);
    out.push_str(">\n");

    out.push_str("<p class=\"url\"><code>");
    push_escaped(out, &mask_url(&request.url));
    out.push_str("</code></p>\n");

    let headers: Vec<&KeyValue> = request
        .headers
        .iter()
        .filter(|h| h.enabled && !h.key.is_empty())
        .collect();
    if !headers.is_empty() {
        out.push_str("<div class=\"label\">Headers</div>\n");
        let rows: Vec<(&str, String)> = headers
            .iter()
            .map(|h| (h.key.as_str(), if is_secret_header(h) { MASKED.to_string() } else { h.value.clone() }))
            .collect();
        append_table(out, &["Name", "Value"], &rows);
    }

    if let Some(auth) = &request.auth {
        append_auth(out, auth);
    }

    if !request.body.is_empty() {
        out.push_str("<div class=\"label\">Body</div>\n");
        out.push_str("<pre>");
        push_escaped(out, &request.body);
        out.push_str("</pre>\n");
    }

    out.push_str("</article>\n");
}

fn append_auth(out: &mut String, auth: &RequestAuth) {
    if auth.kind == auth_kinds::NONE || auth.kind.is_empty() {
        return;
    }

    out.push_str("<div class=\"label\">Authentication</div>\n<dl>\n");
    out.push_str("<dt>Type</dt><dd>");
    push_escaped(out, &auth.kind);
    out.push_str("</dd>\n");

    match auth.kind.as_str() {
        auth_kinds::BEARER => append_field(out, "Token", &mask_credential(&auth.token)),
        auth_kinds::BASIC => {
            append_field(out, "Username", &auth.username);
            append_field(out, "Password", &mask_credential(&auth.password));
        }
        auth_kinds::API_KEY => {
            append_field(out, "Key name", &auth.key_name);
            let sent_in = if auth.key_in_query { "query parameter" } else { "header" };
            append_field(out, "Sent in", sent_in);
            append_field(out, "Key value", &mask_credential(&auth.key_value));
        }
        _ => {}
    }

    out.push_str("</dl>\n");
}

fn append_field(out: &mut String, label: &str, value: &str) {
    if value.is_empty() {
        return;
    }

    out.push_str("<dt>");
    push_escaped(out, label);
    out.push_str("</dt><dd>");
    push_escaped(out, value);
    out.push_str("</dd>\n");
}

fn append_table(out: &mut String, columns: &[&str], rows: &[(&str, String)]) {
    out.push_str("<table>\n<thead><tr>");
    for column in columns {
        out.push_str("<th>");
        push_escaped(out, column);
        out.push_str("</th>");
    }
    out.push_str("</tr></thead>\n<tbody>\n");
    for (name, value) in rows {
        out.push_str("<tr><td><code>");
        push_escaped(out, name);
        out.push_str("</code></td><td><code>");
        push_escaped(out, value);
        out.push_str("</code></td></tr>\n");
    }
    out.push_str("</tbody>\n</table>\n");
}

/// A credential is hidden when it holds a real value. A `{{placeholder}}` names a variable and is safe to show.
fn mask_credential(value: &str) -> String {
    if is_literal(value) { MASKED.to_string() } else { value.to_string() }
}

fn is_secret_header(header: &KeyValue) -> bool {
    header.is_secret || (is_sensitive_header(&header.key) && is_literal(&header.value))
}

/// Masks sensitive query values, keeping the parameter name and any placeholder.
fn mask_url(url: &str) -> String {
    SENSITIVE_QUERY_PARAMETER
        .replace_all(url, |caps: &Captures| {
            if is_literal(&caps[2]) {
                // The separator is `?` or `&`, both one ASCII byte.
                format!("{}{}={}", &caps[0][..1], &caps[1], MASKED)
            } else {
                caps[0].to_string()
            }
        })
        .into_owned()
}

/// True when the text holds an actual value, not just `{{placeholders}}` that are safe to share.
/// Copied from `SecretRules.IsLiteral` in `WorkspaceSecrets.cs`.
fn is_literal(value: &str) -> bool {
    !PLACEHOLDER.replace_all(value, "").trim().is_empty()
}

/// Copied from `SecretRules.IsSensitiveHeader` in `WorkspaceSecrets.cs`.
fn is_sensitive_header(name: &str) -> bool {
    let name = name.trim();
    SENSITIVE_HEADERS.iter().any(|known| known.eq_ignore_ascii_case(name))
}

fn heading(level: usize) -> String {
    format!("h{}", level.min(LAST_HEADING_LEVEL))
}

/// Escapes the five characters that are special in HTML text and in quoted attributes.
fn push_escaped(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
}
