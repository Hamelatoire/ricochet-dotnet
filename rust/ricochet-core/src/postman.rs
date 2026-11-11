//! Port of `PostmanImporter.cs` and `PostmanImporter.Dump.cs` (ADR 0002, ADR 0038). Converts Postman v2.x
//! Collections, Environments and Globals into the models, and lists in an [`ImportResult`] anything that
//! could not be carried over, so nothing is dropped silently.
//!
//! A Request's own test script is translated into Assertions where it can be (`postman_test_translator`,
//! ADR 0037). Any other script, and a test script that is not fully understood, is kept as a script and
//! reported as not translated. Scripts are never run.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io::{Read, Seek};
use std::sync::OnceLock;

use regex::Regex;
use serde_json::{Map, Value};

use crate::builtins;
use crate::models::{
    ApiEnvironment, ApiRequest, Collection, CollectionItem, KeyValue, RequestAuth, StoredScript,
    auth_kinds,
};
use crate::postman_test_translator::try_translate;

/// The most unpacked data a data dump may hold, so a small ZIP cannot fill memory.
pub const MAX_DUMP_BYTES: u64 = 100 * 1024 * 1024;

const MULTIPART_BOUNDARY: &str = "----RicochetFormBoundary";

/// A file that cannot be read as a Postman export. The message is shown to the person as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostmanImportError {
    message: String,
}

impl PostmanImportError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for PostmanImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PostmanImportError {}

/// The kinds of thing an import reports. The labels are the report headings shown to the person.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportCategory {
    Scripts,
    DynamicVariables,
    UnsupportedAuth,
    DroppedBody,
    DroppedData,
    Secrets,
}

impl ImportCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Scripts => "Scripts kept but not run",
            Self::DynamicVariables => "Dynamic variables that will not be resolved",
            Self::UnsupportedAuth => "Auth that was not imported",
            Self::DroppedBody => "Body content that was not imported",
            Self::DroppedData => "Other data that was not imported",
            Self::Secrets => "Variables marked as Secret, so kept out of your files",
        }
    }
}

/// Something an import could not carry over, or that behaves differently here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportIssue {
    pub category: ImportCategory,
    pub location: String,
    pub detail: String,
}

/// What one export converted to. Exactly one of `collection`, `environment` and `globals` is set.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImportResult {
    pub summary: String,
    pub collection: Option<Collection>,
    pub environment: Option<ApiEnvironment>,
    pub globals: Option<Vec<KeyValue>>,
    pub issues: Vec<ImportIssue>,
}

impl ImportResult {
    /// The summary, then each category with its issues, in the order the categories first appear.
    pub fn to_report_text(&self) -> String {
        let mut text = format!("{}\n", self.summary);
        let mut categories: Vec<ImportCategory> = Vec::new();
        for issue in &self.issues {
            if !categories.contains(&issue.category) {
                categories.push(issue.category);
            }
        }

        for category in categories {
            let group: Vec<&ImportIssue> = self
                .issues
                .iter()
                .filter(|i| i.category == category)
                .collect();
            text.push_str(&format!("\n{} ({}):\n", category.label(), group.len()));
            for issue in group {
                if issue.detail.is_empty() {
                    text.push_str(&format!("  {}\n", issue.location));
                } else {
                    text.push_str(&format!("  {}: {}\n", issue.location, issue.detail));
                }
            }
        }

        text
    }
}

/// Converts one exported Postman file. `file_name` names the Collection or Environment when the file has no name.
/// Anything not carried over is listed in the result's issues.
pub fn import(json: &str, file_name: &str) -> Result<ImportResult, PostmanImportError> {
    let root: Value = serde_json::from_str(json)
        .map_err(|e| PostmanImportError::new(format!("Not valid JSON: {e}")))?;
    if root.is_object() {
        if has(&root, "info") && has(&root, "item") {
            return CollectionReader::default().read(&root, file_name);
        }
        if let Some(values) = array(&root, "values") {
            return Ok(read_variable_file(&root, values, file_name));
        }
        if has(&root, "collections") || has(&root, "requests") {
            return Err(PostmanImportError::new(
                "This uses Postman's older v1 or full-data-dump format. Export each collection as v2.1 and each environment separately.",
            ));
        }
    }

    Err(PostmanImportError::new(
        "This is not a Postman collection, environment or globals export.",
    ))
}

fn read_variable_file(root: &Value, values: &[Value], file_name: &str) -> ImportResult {
    let name = non_empty(text(root, "name")).unwrap_or_else(|| file_stem(file_name));
    let is_globals = text(root, "_postman_variable_scope").as_deref() == Some("globals");
    let location = if is_globals {
        "Globals".to_string()
    } else {
        name.clone()
    };

    let mut issues = Vec::new();
    let mut variables = Vec::new();
    for value in values {
        let Some(key) = non_empty(text(value, "key")) else {
            continue;
        };

        // Only an explicit `false` disables a variable, as in the C# reader.
        let enabled = value.get("enabled") != Some(&Value::Bool(false));
        variables.push(KeyValue {
            key: key.clone(),
            value: text_or_empty(value, "value"),
            enabled,
            is_secret: is_secret_type(value),
            ..KeyValue::default()
        });
        note_secret(value, &key, &location, &mut issues);
    }

    if is_globals {
        ImportResult {
            summary: format!("Imported {}.", count(variables.len(), "global variable")),
            globals: Some(variables),
            issues,
            ..ImportResult::default()
        }
    } else {
        let summary = format!(
            "Imported environment \"{name}\" with {}.",
            count(variables.len(), "variable")
        );
        ImportResult {
            summary,
            environment: Some(ApiEnvironment {
                name,
                variables,
                ..ApiEnvironment::default()
            }),
            issues,
            ..ImportResult::default()
        }
    }
}

fn is_secret_type(variable: &Value) -> bool {
    text(variable, "type").as_deref() == Some("secret")
}

fn note_secret(variable: &Value, key: &str, location: &str, issues: &mut Vec<ImportIssue>) {
    if !is_secret_type(variable) {
        return;
    }

    let detail = if text_or_empty(variable, "value").is_empty() {
        format!("{key} (the export has no value, so set it again)")
    } else {
        key.to_string()
    };
    issues.push(issue(ImportCategory::Secrets, location, detail));
}

/// Reads one Collection. The counters feed the summary and the issues about things not imported.
#[derive(Default)]
struct CollectionReader {
    issues: Vec<ImportIssue>,
    requests: usize,
    folders: usize,
    examples: usize,
    descriptions: usize,
}

impl CollectionReader {
    fn read(&mut self, root: &Value, file_name: &str) -> Result<ImportResult, PostmanImportError> {
        let null = Value::Null;
        let info = root.get("info").unwrap_or(&null);
        let schema = text_or_empty(info, "schema");
        if !schema.is_empty() && !schema.contains("/v2.") {
            return Err(PostmanImportError::new(format!(
                "Unsupported collection schema: {schema}. Export the collection as v2.1."
            )));
        }

        let name = non_empty(text(info, "name")).unwrap_or_else(|| file_stem(file_name));
        if has(info, "description") {
            self.descriptions += 1;
        }

        let auth = self.read_auth(root, &name, None);
        let items = self.read_items(root, &name, auth.as_ref());
        let variables = self.read_collection_variables(root, &name);
        let scripts = self.read_scripts(root, &name, None);

        if self.examples > 0 {
            self.issues.push(issue(
                ImportCategory::DroppedData,
                count(self.examples, "saved example response"),
                "",
            ));
        }
        if self.descriptions > 0 {
            self.issues.push(issue(
                ImportCategory::DroppedData,
                count(self.descriptions, "description"),
                "",
            ));
        }

        let summary = format!(
            "Imported collection \"{name}\": {}, {}, {}.",
            count(self.requests, "request"),
            count(self.folders, "folder"),
            count(variables.len(), "variable"),
        );
        Ok(ImportResult {
            summary,
            collection: Some(Collection {
                name,
                items,
                variables,
                scripts,
                ..Collection::default()
            }),
            issues: std::mem::take(&mut self.issues),
            ..ImportResult::default()
        })
    }

    fn read_collection_variables(&mut self, root: &Value, location: &str) -> Vec<KeyValue> {
        let mut variables = Vec::new();
        for variable in elements(root, "variable") {
            let Some(key) = non_empty(text(variable, "key"))
                .or_else(|| text(variable, "id"))
                .filter(|k| !k.is_empty())
            else {
                continue;
            };

            variables.push(KeyValue {
                key: key.clone(),
                value: text_or_empty(variable, "value"),
                enabled: !is_true(variable, "disabled"),
                is_secret: is_secret_type(variable),
                ..KeyValue::default()
            });
            note_secret(variable, &key, location, &mut self.issues);
        }

        variables
    }

    fn read_items(
        &mut self,
        parent: &Value,
        parent_path: &str,
        inherited: Option<&RequestAuth>,
    ) -> Vec<CollectionItem> {
        let mut items = Vec::new();
        for item in elements(parent, "item") {
            let name = text(item, "name").unwrap_or_else(|| "Untitled".to_string());
            let path = format!("{parent_path} / {name}");

            if has(item, "request") {
                items.push(self.read_request_item(item, name, &path, inherited));
            } else if has(item, "item") {
                self.folders += 1;
                if has(item, "description") {
                    self.descriptions += 1;
                }

                let auth = self.read_auth(item, &path, inherited);
                let children = self.read_items(item, &path, auth.as_ref());
                let scripts = self.read_scripts(item, &path, None);
                items.push(CollectionItem {
                    name,
                    children,
                    scripts,
                    ..CollectionItem::default()
                });
            } else {
                self.issues.push(issue(
                    ImportCategory::DroppedData,
                    path,
                    "not a request or a folder",
                ));
            }
        }

        items
    }

    fn read_request_item(
        &mut self,
        item: &Value,
        name: String,
        path: &str,
        inherited: Option<&RequestAuth>,
    ) -> CollectionItem {
        self.requests += 1;
        self.examples += elements(item, "response").len();

        let source = &item["request"];
        let mut request = if let Value::String(url) = source {
            ApiRequest {
                name: name.clone(),
                url: url.clone(),
                auth: inherited.cloned(),
                ..ApiRequest::default()
            }
        } else {
            self.read_request(source, &name, path, inherited)
        };

        self.note_dynamic_variables(&request, path);
        let scripts = self.read_scripts(item, path, Some(&mut request));
        CollectionItem {
            name,
            request: Some(request),
            scripts,
            ..CollectionItem::default()
        }
    }

    fn read_request(
        &mut self,
        source: &Value,
        name: &str,
        path: &str,
        inherited: Option<&RequestAuth>,
    ) -> ApiRequest {
        if has(source, "description") {
            self.descriptions += 1;
        }

        let mut headers = read_headers(source);
        let body = self.read_body(source, path, &mut headers);
        let url = self.read_url(source, path);
        let auth = self.read_auth(source, path, inherited);
        ApiRequest {
            name: name.to_string(),
            method: text(source, "method")
                .unwrap_or_else(|| "GET".to_string())
                .to_uppercase(),
            url,
            headers,
            body,
            auth,
            ..ApiRequest::default()
        }
    }

    fn read_url(&mut self, request: &Value, path: &str) -> String {
        match request.get("url") {
            Some(Value::String(url)) => url.clone(),
            Some(url @ Value::Object(_)) => self.read_url_object(url, path),
            _ => String::new(),
        }
    }

    fn read_url_object(&mut self, url: &Value, path: &str) -> String {
        let mut raw = non_empty(text(url, "raw")).unwrap_or_else(|| build_url(url));

        for query in elements(url, "query") {
            if is_true(query, "disabled") {
                let detail = format!(
                    "disabled query parameter \"{}\"",
                    text_or_empty(query, "key")
                );
                self.issues
                    .push(issue(ImportCategory::DroppedData, path, detail));
            }
        }

        for variable in elements(url, "variable") {
            let key = text_or_empty(variable, "key");
            let value = text_or_empty(variable, "value");
            if !key.is_empty() && !value.is_empty() {
                raw = replace_path_variable(&raw, &key, &value);
            }
        }

        raw
    }

    fn read_body(&mut self, request: &Value, path: &str, headers: &mut Vec<KeyValue>) -> String {
        let Some(body) = request.get("body").filter(|b| b.is_object()) else {
            return String::new();
        };

        let mode = text_or_empty(body, "mode");
        if is_true(body, "disabled") {
            self.issues.push(issue(
                ImportCategory::DroppedBody,
                path,
                format!("disabled {mode} body"),
            ));
            return String::new();
        }

        match mode.as_str() {
            "" => String::new(),
            "raw" => {
                let language = body
                    .get("options")
                    .and_then(|options| options.get("raw"))
                    .and_then(|raw| text(raw, "language"));
                let content_type = match language.as_deref() {
                    Some("json") => Some("application/json"),
                    Some("xml") => Some("application/xml"),
                    Some("html") => Some("text/html"),
                    Some("javascript") => Some("application/javascript"),
                    _ => None,
                };
                if let Some(content_type) = content_type {
                    add_content_type(headers, content_type);
                }
                text_or_empty(body, "raw")
            }
            "urlencoded" => self.read_url_encoded(body, path, headers),
            "formdata" => self.read_form_data(body, path, headers),
            "graphql" => self.read_graphql(body, path, headers),
            "file" => {
                self.issues
                    .push(issue(ImportCategory::DroppedBody, path, "file upload"));
                String::new()
            }
            other => {
                self.issues.push(issue(
                    ImportCategory::DroppedBody,
                    path,
                    format!("unknown body type \"{other}\""),
                ));
                String::new()
            }
        }
    }

    fn read_url_encoded(
        &mut self,
        body: &Value,
        path: &str,
        headers: &mut Vec<KeyValue>,
    ) -> String {
        let mut pairs = Vec::new();
        for field in elements(body, "urlencoded") {
            if is_true(field, "disabled") {
                let detail = format!("disabled form field \"{}\"", text_or_empty(field, "key"));
                self.issues
                    .push(issue(ImportCategory::DroppedBody, path, detail));
            } else {
                pairs.push(format!(
                    "{}={}",
                    escape_keeping_placeholders(&text_or_empty(field, "key")),
                    escape_keeping_placeholders(&text_or_empty(field, "value")),
                ));
            }
        }

        if pairs.is_empty() {
            return String::new();
        }

        add_content_type(headers, "application/x-www-form-urlencoded");
        pairs.join("&")
    }

    // Our body is text, so text fields are written as literal multipart content that stays visible and editable.
    fn read_form_data(&mut self, body: &Value, path: &str, headers: &mut Vec<KeyValue>) -> String {
        let mut multipart = String::new();
        for field in elements(body, "formdata") {
            let key = text_or_empty(field, "key");
            if text(field, "type").as_deref() == Some("file") {
                self.issues.push(issue(
                    ImportCategory::DroppedBody,
                    path,
                    format!("file form field \"{key}\""),
                ));
            } else if is_true(field, "disabled") {
                self.issues.push(issue(
                    ImportCategory::DroppedBody,
                    path,
                    format!("disabled form field \"{key}\""),
                ));
            } else {
                multipart.push_str(&format!(
                    "--{MULTIPART_BOUNDARY}\r\nContent-Disposition: form-data; name=\"{key}\"\r\n"
                ));
                if let Some(part_type) = non_empty(text(field, "contentType")) {
                    multipart.push_str(&format!("Content-Type: {part_type}\r\n"));
                }
                multipart.push_str(&format!("\r\n{}\r\n", text_or_empty(field, "value")));
            }
        }

        if multipart.is_empty() {
            return String::new();
        }

        add_content_type(
            headers,
            &format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}"),
        );
        multipart.push_str(&format!("--{MULTIPART_BOUNDARY}--\r\n"));
        multipart
    }

    fn read_graphql(&mut self, body: &Value, path: &str, headers: &mut Vec<KeyValue>) -> String {
        let Some(graphql) = body.get("graphql").filter(|g| g.is_object()) else {
            return String::new();
        };

        let mut output = Map::new();
        output.insert(
            "query".to_string(),
            Value::String(text_or_empty(graphql, "query")),
        );
        if let Some(variables) = text(graphql, "variables").filter(|v| !v.trim().is_empty()) {
            match serde_json::from_str::<Value>(&variables) {
                Ok(parsed) => {
                    output.insert("variables".to_string(), parsed);
                }
                Err(_) => self.issues.push(issue(
                    ImportCategory::DroppedBody,
                    path,
                    "GraphQL variables that are not valid JSON",
                )),
            }
        }

        add_content_type(headers, "application/json");
        serde_json::to_string_pretty(&Value::Object(output)).unwrap_or_default()
    }

    // Inherited auth is resolved here, so each request holds the auth it will actually use.
    fn read_auth(
        &mut self,
        owner: &Value,
        location: &str,
        inherited: Option<&RequestAuth>,
    ) -> Option<RequestAuth> {
        let Some(auth) = owner.get("auth").filter(|a| a.is_object()) else {
            return inherited.cloned();
        };

        let kind = text_or_empty(auth, "type");
        let values = read_auth_values(auth, &kind);
        let value = |key: &str| values.get(key).cloned().unwrap_or_default();

        match kind.as_str() {
            "inherit" | "" => inherited.cloned(),
            "noauth" => None,
            auth_kinds::BEARER => Some(RequestAuth {
                kind: auth_kinds::BEARER.to_string(),
                token: value("token"),
                ..RequestAuth::default()
            }),
            auth_kinds::BASIC => Some(RequestAuth {
                kind: auth_kinds::BASIC.to_string(),
                username: value("username"),
                password: value("password"),
                ..RequestAuth::default()
            }),
            auth_kinds::API_KEY => Some(RequestAuth {
                kind: auth_kinds::API_KEY.to_string(),
                key_name: value("key"),
                key_value: value("value"),
                key_in_query: value("in") == "query",
                ..RequestAuth::default()
            }),
            _ => {
                self.issues.push(issue(
                    ImportCategory::UnsupportedAuth,
                    location,
                    kind.clone(),
                ));
                None
            }
        }
    }

    // Only a Request's own test script is translated (ADR 0037). Folder and Collection scripts, and any test
    // script that is not fully understood, are kept and reported. `request` is None for those owners.
    fn read_scripts(
        &mut self,
        owner: &Value,
        location: &str,
        mut request: Option<&mut ApiRequest>,
    ) -> Vec<StoredScript> {
        let mut scripts = Vec::new();
        for entry in elements(owner, "event") {
            let Some(script) = entry.get("script").filter(|s| s.is_object()) else {
                continue;
            };

            let source = read_exec(script);
            if source.trim().is_empty() {
                continue; // Postman exports empty script stubs for many requests.
            }

            let listen = text(entry, "listen").unwrap_or_else(|| "script".to_string());
            let mut detail = listen.clone();
            if listen == "test"
                && let Some(request) = request.as_deref_mut()
            {
                match try_translate(&source) {
                    Ok(checks) => {
                        let mut all = request.assertions.take().unwrap_or_default();
                        all.extend(checks);
                        request.assertions = Some(all);
                        continue;
                    }
                    Err(reason) => detail = format!("{listen}, not translated: {reason}"),
                }
            }

            scripts.push(StoredScript { listen, source });
            self.issues
                .push(issue(ImportCategory::Scripts, location, detail));
        }

        scripts
    }

    fn note_dynamic_variables(&mut self, request: &ApiRequest, path: &str) {
        let mut texts: Vec<&str> = vec![request.url.as_str(), request.body.as_str()];
        texts.extend(request.headers.iter().map(|h| h.value.as_str()));
        if let Some(auth) = &request.auth {
            texts.extend([
                auth.token.as_str(),
                auth.username.as_str(),
                auth.password.as_str(),
                auth.key_value.as_str(),
            ]);
        }

        // The names with a built-in function (ADR 0008) now resolve, so only the others are reported.
        let mut names: Vec<String> = Vec::new();
        for text in texts {
            for captures in dynamic_variable().captures_iter(text) {
                let name = captures[1].to_string();
                if !builtins::is_postman_alias(&name) && !names.contains(&name) {
                    names.push(name);
                }
            }
        }

        if !names.is_empty() {
            self.issues.push(issue(
                ImportCategory::DynamicVariables,
                path,
                names.join(", "),
            ));
        }
    }
}

fn read_headers(request: &Value) -> Vec<KeyValue> {
    let mut headers = Vec::new();
    let Some(source) = request.get("header") else {
        return headers;
    };

    match source {
        // A plain string holds one "Name: value" header per line.
        Value::String(lines) => {
            for line in lines.split('\n').filter(|line| !line.is_empty()) {
                if let Some(colon) = line.find(':').filter(|&colon| colon > 0) {
                    headers.push(KeyValue::new(
                        line[..colon].trim(),
                        line[colon + 1..].trim(),
                    ));
                }
            }
        }
        Value::Array(entries) => {
            for header in entries {
                if let Some(key) = non_empty(text(header, "key")) {
                    headers.push(KeyValue {
                        key,
                        value: text_or_empty(header, "value"),
                        enabled: !is_true(header, "disabled"),
                        ..KeyValue::default()
                    });
                }
            }
        }
        _ => {}
    }

    headers
}

fn build_url(url: &Value) -> String {
    fn join_parts(parent: &Value, name: &str, separator: &str) -> String {
        match parent.get(name) {
            Some(Value::String(part)) => part.clone(),
            Some(Value::Array(parts)) => parts
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(separator),
            _ => String::new(),
        }
    }

    let mut built = String::new();
    if let Some(protocol) = non_empty(text(url, "protocol")) {
        built.push_str(&protocol);
        built.push_str("://");
    }

    built.push_str(&join_parts(url, "host", "."));
    if let Some(port) = non_empty(text(url, "port")) {
        built.push(':');
        built.push_str(&port);
    }

    let path = join_parts(url, "path", "/");
    if !path.is_empty() {
        built.push('/');
        built.push_str(path.trim_start_matches('/'));
    }

    let query: Vec<String> = elements(url, "query")
        .iter()
        .filter(|q| !is_true(q, "disabled"))
        .map(|q| format!("{}={}", text_or_empty(q, "key"), text_or_empty(q, "value")))
        .collect();
    if !query.is_empty() {
        built.push('?');
        built.push_str(&query.join("&"));
    }

    built
}

/// Replaces `:key` in a path segment (after a `/`, before `/`, `?`, `#` or the end) with the value.
fn replace_path_variable(raw: &str, key: &str, value: &str) -> String {
    let needle = format!("/:{key}");
    let mut out = String::with_capacity(raw.len());
    let mut last = 0;
    for (start, _) in raw.match_indices(&needle) {
        let end = start + needle.len();
        if matches!(raw[end..].chars().next(), None | Some('/' | '?' | '#')) {
            out.push_str(&raw[last..start]);
            out.push('/');
            out.push_str(value);
            last = end;
        }
    }

    out.push_str(&raw[last..]);
    out
}

/// Escapes what needs escaping in a value, but keeps `{{placeholders}}` as they are so they still resolve.
fn escape_keeping_placeholders(text: &str) -> String {
    let mut out = String::new();
    let mut last = 0;
    for found in placeholder().find_iter(text) {
        out.push_str(&escape_segment(&text[last..found.start()]));
        out.push_str(found.as_str());
        last = found.end();
    }

    out.push_str(&escape_segment(&text[last..]));
    out
}

fn escape_segment(segment: &str) -> String {
    // Same rule as the C# reader: a segment that starts with "{{" is kept as written.
    if segment.starts_with("{{") {
        segment.to_string()
    } else {
        escape_data_string(segment)
    }
}

/// Percent-encodes everything except the RFC 3986 unreserved characters, as .NET's `Uri.EscapeDataString` does.
fn escape_data_string(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }

    out
}

/// Reads the auth values of one kind. v2.1 stores them as a list of key/value objects, v2.0 as one plain object.
fn read_auth_values(auth: &Value, kind: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();
    if kind.is_empty() {
        return values;
    }

    match auth.get(kind) {
        Some(Value::Array(entries)) => {
            for entry in entries {
                if let Some(key) = non_empty(text(entry, "key")) {
                    values.insert(key, text_or_empty(entry, "value"));
                }
            }
        }
        Some(Value::Object(fields)) => {
            for (name, value) in fields {
                values.insert(name.clone(), as_text(value).unwrap_or_default());
            }
        }
        _ => {}
    }

    values
}

fn read_exec(script: &Value) -> String {
    match script.get("exec") {
        Some(Value::String(source)) => source.clone(),
        Some(Value::Array(lines)) => lines
            .iter()
            .map(|line| as_text(line).unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn add_content_type(headers: &mut Vec<KeyValue>, value: &str) {
    if !headers
        .iter()
        .any(|h| h.enabled && h.key.eq_ignore_ascii_case("Content-Type"))
    {
        headers.push(KeyValue::new("Content-Type", value));
    }
}

fn placeholder() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"\{\{[^{}]+\}\}").expect("valid pattern"))
}

fn dynamic_variable() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"\{\{\s*(\$[^{}\s]+)\s*\}\}").expect("valid pattern"))
}

fn issue(
    category: ImportCategory,
    location: impl Into<String>,
    detail: impl Into<String>,
) -> ImportIssue {
    ImportIssue {
        category,
        location: location.into(),
        detail: detail.into(),
    }
}

fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// The file name without its extension. Like .NET's `Path.GetFileNameWithoutExtension`, a name such as `.json` gives "".
fn file_stem(file_name: &str) -> String {
    let base = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    match base.rfind('.') {
        Some(dot) => base[..dot].to_string(),
        None => base.to_string(),
    }
}

fn has(element: &Value, name: &str) -> bool {
    element.get(name).is_some_and(|value| !value.is_null())
}

fn array<'a>(element: &'a Value, name: &str) -> Option<&'a [Value]> {
    element
        .get(name)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
}

fn elements<'a>(element: &'a Value, name: &str) -> &'a [Value] {
    array(element, name).unwrap_or(&[])
}

fn is_true(element: &Value, name: &str) -> bool {
    element.get(name) == Some(&Value::Bool(true))
}

fn text(element: &Value, name: &str) -> Option<String> {
    element.get(name).and_then(as_text)
}

fn text_or_empty(element: &Value, name: &str) -> String {
    text(element, name).unwrap_or_default()
}

fn non_empty(text: Option<String>) -> Option<String> {
    text.filter(|t| !t.is_empty())
}

// Postman allows numbers and booleans where a string is usual, such as variable values.
fn as_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

// ---- Data dumps (PostmanImporter.Dump.cs, ADR 0038) ----

/// The kind of one export inside a data dump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Collection,
    Environment,
    Globals,
}

/// One export inside a data dump, already read, so choosing it needs no second read.
#[derive(Debug, Clone, PartialEq)]
pub struct DumpEntry {
    /// The path of the entry inside the ZIP. Unique within a dump, so it identifies the entry in a selection.
    pub file_name: String,
    pub name: String,
    pub kind: EntryKind,
    pub result: ImportResult,
}

/// Reads every export in a Postman data dump, a ZIP of collection, environment and globals exports.
/// Nothing is imported until the caller chooses from the returned list.
pub fn read_dump<R: Read + Seek>(dump: R) -> Result<Vec<DumpEntry>, PostmanImportError> {
    let mut archive = zip::ZipArchive::new(dump).map_err(|_| {
        PostmanImportError::new("This is not a Postman data dump, because it is not a ZIP file.")
    })?;

    let mut files: Vec<(usize, String, u64)> = Vec::new();
    for index in 0..archive.len() {
        let entry = archive.by_index_raw(index).map_err(|e| {
            PostmanImportError::new(format!("This data dump could not be read: {e}"))
        })?;
        let name = entry
            .name()
            .map_err(|e| PostmanImportError::new(format!("This data dump could not be read: {e}")))?
            .to_string();
        if !name.ends_with('/') {
            files.push((index, name, entry.size()));
        }
    }

    if files.is_empty() {
        return Err(PostmanImportError::new("This data dump holds no files."));
    }
    if files.iter().map(|(_, _, size)| size).sum::<u64>() > MAX_DUMP_BYTES {
        return Err(PostmanImportError::new(
            "This data dump is too large to read. Export the collections you need in smaller files.",
        ));
    }

    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for (index, name, _) in files {
        if !seen.insert(name.clone()) {
            return Err(PostmanImportError::new(format!(
                "{name} is listed more than once in this data dump."
            )));
        }
        if !name.to_ascii_lowercase().ends_with(".json") {
            return Err(PostmanImportError::new(format!(
                "{name} is not a Postman export. A data dump may hold only .json exports."
            )));
        }

        let mut bytes = Vec::new();
        archive
            .by_index(index)
            .and_then(|mut file| {
                file.read_to_end(&mut bytes)
                    .map_err(zip::result::ZipError::Io)
            })
            .map_err(|e| PostmanImportError::new(format!("{name} could not be read: {e}")))?;
        let decoded = String::from_utf8_lossy(&bytes);
        let json = decoded.trim_start_matches('\u{feff}');

        let result = import(json, &name)
            .map_err(|e| PostmanImportError::new(format!("{name}: {}", e.message())))?;
        let entry_name = name_of(&result);
        let kind = kind_of(&result);
        entries.push(DumpEntry {
            file_name: name,
            name: entry_name,
            kind,
            result,
        });
    }

    Ok(entries)
}

/// The results for the entries whose file names are selected, in dump order. Names not in the listing are ignored.
pub fn selected<'a>(
    entries: &'a [DumpEntry],
    selected_file_names: &HashSet<String>,
) -> Vec<&'a ImportResult> {
    entries
        .iter()
        .filter(|entry| selected_file_names.contains(&entry.file_name))
        .map(|entry| &entry.result)
        .collect()
}

fn name_of(result: &ImportResult) -> String {
    result
        .collection
        .as_ref()
        .map(|c| c.name.clone())
        .or_else(|| result.environment.as_ref().map(|e| e.name.clone()))
        .unwrap_or_else(|| "Globals".to_string())
}

fn kind_of(result: &ImportResult) -> EntryKind {
    if result.collection.is_some() {
        EntryKind::Collection
    } else if result.environment.is_some() {
        EntryKind::Environment
    } else {
        EntryKind::Globals
    }
}
