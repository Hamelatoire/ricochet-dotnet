//! Port of `InsomniaImporter.cs` (ADR 0033). Converts Insomnia format 4 JSON exports into the Core model.
//! Anything not carried over is reported as an [`ImportIssue`]. Insomnia scripts are not read, as in C#,
//! so none are stored. YAML exports are not read (still open in ADR 0033).
//!
//! Known differences from C#: JSON with trailing commas or comments is refused, and nesting deeper than
//! serde_json's limit of 128 is refused (C# allows 512). Numbers are written in serde_json's form, so `1e3`
//! becomes `1000.0`. Only the whitespace inside object and array variable values can differ.
//!
//! `ImportIssue` and `ImportCategories` are defined here for now. The Postman port should move them into a
//! shared module, and this file can then use that module.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::OnceLock;

use regex::{Captures, NoExpand, Regex};
use serde_json::Value;

use crate::builtins;
use crate::models::{ApiEnvironment, ApiRequest, Collection, CollectionItem, KeyValue, RequestAuth, auth_kinds};

const EXPORT_FORMAT_PROPERTY: &str = "__export_format";
const SUPPORTED_FORMAT: i64 = 4;
const MULTIPART_BOUNDARY: &str = "----RicochetFormBoundary";

/// The categories an import report groups its issues under. The strings match C#'s `ImportCategories`.
pub struct ImportCategories;

impl ImportCategories {
    pub const SCRIPTS: &'static str = "Scripts kept but not run";
    pub const DYNAMIC_VARIABLES: &'static str = "Dynamic variables that will not be resolved";
    pub const UNSUPPORTED_AUTH: &'static str = "Auth that was not imported";
    pub const DROPPED_BODY: &'static str = "Body content that was not imported";
    pub const DROPPED_DATA: &'static str = "Other data that was not imported";
    pub const SECRETS: &'static str = "Variables marked as Secret, so kept out of your files";
}

/// Something an import could not carry over, or that behaves differently here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportIssue {
    pub category: &'static str,
    pub location: String,
    pub detail: String,
}

impl ImportIssue {
    pub fn new(category: &'static str, location: impl Into<String>, detail: impl Into<String>) -> Self {
        Self { category, location: location.into(), detail: detail.into() }
    }
}

/// The export could not be read at all. The message is shown to the user as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsomniaImportError {
    pub message: String,
}

impl InsomniaImportError {
    fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

impl fmt::Display for InsomniaImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for InsomniaImportError {}

/// One Insomnia workspace. Its base environment becomes the Collection's Variables, and each sub-environment
/// becomes an Environment.
#[derive(Debug, Clone, PartialEq)]
pub struct InsomniaWorkspaceImport {
    pub collection: Collection,
    pub environments: Vec<ApiEnvironment>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InsomniaImportResult {
    pub summary: String,
    pub workspaces: Vec<InsomniaWorkspaceImport>,
    pub issues: Vec<ImportIssue>,
}

impl InsomniaImportResult {
    /// The summary, then the issues grouped by category in the order they first appear.
    pub fn to_report_text(&self) -> String {
        let mut text = format!("{}\n", self.summary);
        let mut groups: Vec<(&'static str, Vec<&ImportIssue>)> = Vec::new();
        for issue in &self.issues {
            match groups.iter_mut().find(|(category, _)| *category == issue.category) {
                Some((_, list)) => list.push(issue),
                None => groups.push((issue.category, vec![issue])),
            }
        }

        for (category, group) in groups {
            text.push_str(&format!("\n{category} ({}):\n", group.len()));
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

/// Reads an Insomnia export. `file_name` names the Collection when the workspace has no name.
///
/// # Errors
/// Returns an error when the text is not JSON, is not an Insomnia export, is not format 4, or has no workspace.
pub fn import(json: &str, file_name: &str) -> Result<InsomniaImportResult, InsomniaImportError> {
    let document: Value = serde_json::from_str(json)
        .map_err(|err| InsomniaImportError::new(format!("Not valid JSON: {err}")))?;

    let (format, resources) = match (document.get(EXPORT_FORMAT_PROPERTY), document.get("resources").and_then(Value::as_array)) {
        (Some(format), Some(resources)) => (format, resources),
        _ => return Err(InsomniaImportError::new("This is not an Insomnia export.")),
    };

    if format.as_i64() != Some(SUPPORTED_FORMAT) {
        return Err(InsomniaImportError::new(format!(
            "This is Insomnia export format {format}, which is not supported. Export the workspace again from a current version of Insomnia, which writes format 4."
        )));
    }

    let tree = ResourceTree::new(resources);
    let workspaces = tree.of_type("workspace");
    if workspaces.is_empty() {
        return Err(InsomniaImportError::new("This Insomnia export has no workspace to import."));
    }

    let mut issues = Vec::new();
    let mut imports = Vec::new();
    let mut summaries = Vec::new();
    for workspace in workspaces {
        let mut reader = WorkspaceReader { tree: &tree, issues: &mut issues, visited: HashSet::new(), requests: 0, folders: 0, descriptions: 0 };
        let (import, summary) = reader.read(workspace, file_name);
        imports.push(import);
        summaries.push(summary);
    }

    Ok(InsomniaImportResult { summary: summaries.join("\n"), workspaces: imports, issues })
}

/// Reads one workspace. Each instance is used for one workspace only, so its counts describe that workspace.
struct WorkspaceReader<'r, 'a> {
    tree: &'r ResourceTree<'a>,
    issues: &'r mut Vec<ImportIssue>,
    visited: HashSet<String>,
    requests: usize,
    folders: usize,
    descriptions: usize,
}

impl<'r, 'a> WorkspaceReader<'r, 'a> {
    fn read(&mut self, workspace: &Value, file_name: &str) -> (InsomniaWorkspaceImport, String) {
        let id = text_or_empty(workspace, "_id");
        let name = non_empty(text_of(workspace, "name")).unwrap_or_else(|| file_stem(file_name));
        self.note_description(workspace);

        let mut workspace_found = Vec::new();
        let auth = self.read_auth(workspace, &name, None, &mut workspace_found);
        self.report_dynamic(&workspace_found, &name);

        let items = self.read_items(&id, &name, auth.as_ref(), &[]);
        let mut environments = Vec::new();
        self.collect_environments(&id, &mut environments);

        let mut base_variables = Vec::new();
        let mut sub_environments = Vec::new();
        for environment in &environments {
            if text_of(environment, "parentId").as_deref() == Some(id.as_str()) {
                let location = text_or_empty(environment, "name");
                base_variables.extend(self.read_variables(environment, &location));
            }
        }
        for environment in &environments {
            if text_of(environment, "parentId").as_deref() != Some(id.as_str()) {
                let location = text_or_empty(environment, "name");
                let name = non_empty(text_of(environment, "name")).unwrap_or_else(|| "Untitled".to_string());
                let variables = self.read_variables(environment, &location);
                sub_environments.push(ApiEnvironment { name, variables, ..ApiEnvironment::default() });
            }
        }

        if self.descriptions > 0 {
            self.issues.push(ImportIssue::new(ImportCategories::DROPPED_DATA, count(self.descriptions, "description"), ""));
        }

        let summary = format!(
            "Imported workspace \"{name}\": {}, {}, {}, {}.",
            count(self.requests, "request"),
            count(self.folders, "folder"),
            count(base_variables.len(), "variable"),
            count(sub_environments.len(), "environment"),
        );
        let collection = Collection { name, items, variables: base_variables, ..Collection::default() };
        (InsomniaWorkspaceImport { collection, environments: sub_environments }, summary)
    }

    // Environments sit under the workspace, and sub-environments under their base environment.
    fn collect_environments(&mut self, parent_id: &str, found: &mut Vec<&'a Value>) {
        let tree = self.tree;
        for &child in tree.children(parent_id) {
            if text_or_empty(child, "_type") != "environment" {
                continue;
            }

            let id = text_or_empty(child, "_id");
            if !id.is_empty() && !self.visited.insert(id.clone()) {
                continue;
            }

            found.push(child);
            if !id.is_empty() {
                self.collect_environments(&id, found);
            }
        }
    }

    fn read_items(&mut self, parent_id: &str, parent_path: &str, inherited_auth: Option<&RequestAuth>, inherited_headers: &[KeyValue]) -> Vec<CollectionItem> {
        let tree = self.tree;
        let mut items = Vec::new();
        for &child in tree.children(parent_id) {
            let kind = text_or_empty(child, "_type");
            if kind == "environment" {
                continue; // Read with the workspace's environments.
            }

            let id = text_or_empty(child, "_id");
            if !id.is_empty() && !self.visited.insert(id.clone()) {
                continue;
            }

            let name = non_empty(text_of(child, "name")).unwrap_or_else(|| "Untitled".to_string());
            let path = format!("{parent_path} / {name}");
            match kind.as_str() {
                "request" => {
                    let request = self.read_request(child, &name, &path, inherited_auth, inherited_headers);
                    items.push(CollectionItem { name, request: Some(request), ..CollectionItem::default() });
                }

                "request_group" => {
                    self.folders += 1;
                    self.note_description(child);
                    let folder_auth = self.read_auth(child, &path, inherited_auth, &mut Vec::new());
                    let mut folder_found = Vec::new();
                    let folder_headers = read_headers(child, &mut folder_found);
                    self.report_dynamic(&folder_found, &path);
                    let passed = inherit(folder_headers.iter().filter(|h| h.enabled).cloned().collect(), inherited_headers);
                    let children = self.read_items(&id, &path, folder_auth.as_ref(), &passed);
                    items.push(CollectionItem { name, children, ..CollectionItem::default() });
                }

                "cookie_jar" => {
                    let cookies = child.get("cookies").and_then(Value::as_array).map_or(0, Vec::len);
                    if cookies > 0 {
                        self.issues.push(ImportIssue::new(ImportCategories::DROPPED_DATA, path, format!("{} not imported", count(cookies, "cookie"))));
                    }
                }

                _ => {
                    self.issues.push(ImportIssue::new(ImportCategories::DROPPED_DATA, path, format!("{kind} not imported")));
                }
            }
        }

        items
    }

    fn read_request(&mut self, source: &Value, name: &str, path: &str, inherited_auth: Option<&RequestAuth>, inherited_headers: &[KeyValue]) -> ApiRequest {
        self.requests += 1;
        self.note_description(source);

        let mut found = Vec::new();
        let mut header_found = Vec::new();
        let own_headers = read_headers(source, &mut header_found);
        let mut headers = inherit(own_headers, inherited_headers);
        found.extend(header_found);

        let url = normalize(&text_or_empty(source, "url"), &mut found);
        for parameter in elements(source, "parameters") {
            if is_true(parameter, "disabled") {
                let parameter_name = text_or_empty(parameter, "name");
                self.issues.push(ImportIssue::new(ImportCategories::DROPPED_DATA, path, format!("disabled query parameter \"{parameter_name}\"")));
            }
        }

        let body = self.read_body(source, path, &mut headers, &mut found);
        let method = text_of(source, "method").unwrap_or_else(|| "GET".to_string()).to_uppercase();
        let auth = self.read_auth(source, path, inherited_auth, &mut found);

        self.report_dynamic(&found, path);
        ApiRequest { name: name.to_string(), method, url, headers, body, auth, ..ApiRequest::default() }
    }

    fn read_body(&mut self, request: &Value, path: &str, headers: &mut Vec<KeyValue>, found: &mut Vec<String>) -> String {
        let Some(body) = request.get("body").filter(|b| b.is_object()) else {
            return String::new();
        };

        if non_empty(text_of(body, "fileName")).is_some() {
            self.issues.push(ImportIssue::new(ImportCategories::DROPPED_BODY, path, "file upload"));
            return String::new();
        }

        let mime = text_or_empty(body, "mimeType");
        match mime.as_str() {
            "application/x-www-form-urlencoded" => return self.read_url_encoded(body, path, headers, found),
            "multipart/form-data" => return self.read_multipart(body, path, headers, found),
            _ => {}
        }

        let text = normalize(&text_or_empty(body, "text"), found);
        if text.is_empty() {
            return String::new();
        }

        if !mime.is_empty() {
            add_content_type(headers, &mime);
        }
        text
    }

    fn read_url_encoded(&mut self, body: &Value, path: &str, headers: &mut Vec<KeyValue>, found: &mut Vec<String>) -> String {
        let mut pairs = Vec::new();
        for field in elements(body, "params") {
            let key = normalize(&text_or_empty(field, "name"), found);
            if is_true(field, "disabled") {
                self.issues.push(ImportIssue::new(ImportCategories::DROPPED_BODY, path, format!("disabled form field \"{key}\"")));
                continue;
            }

            let value = normalize(&text_or_empty(field, "value"), found);
            pairs.push(format!("{}={}", escape_keeping_placeholders(&key), escape_keeping_placeholders(&value)));
        }

        if pairs.is_empty() {
            return String::new();
        }

        add_content_type(headers, "application/x-www-form-urlencoded");
        pairs.join("&")
    }

    // Our body is text, so text fields are written as literal multipart content that stays visible and editable.
    fn read_multipart(&mut self, body: &Value, path: &str, headers: &mut Vec<KeyValue>, found: &mut Vec<String>) -> String {
        let mut multipart = String::new();
        for field in elements(body, "params") {
            let key = text_or_empty(field, "name");
            if text_of(field, "type").as_deref() == Some("file") || non_empty(text_of(field, "fileName")).is_some() {
                self.issues.push(ImportIssue::new(ImportCategories::DROPPED_BODY, path, format!("file form field \"{key}\"")));
            } else if is_true(field, "disabled") {
                self.issues.push(ImportIssue::new(ImportCategories::DROPPED_BODY, path, format!("disabled form field \"{key}\"")));
            } else {
                let value = normalize(&text_or_empty(field, "value"), found);
                multipart.push_str(&format!("--{MULTIPART_BOUNDARY}\r\nContent-Disposition: form-data; name=\"{key}\"\r\n\r\n{value}\r\n"));
            }
        }

        if multipart.is_empty() {
            return String::new();
        }

        add_content_type(headers, &format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}"));
        multipart.push_str(&format!("--{MULTIPART_BOUNDARY}--\r\n"));
        multipart
    }

    // Inherited auth is resolved here, so each request holds the auth it will actually use.
    fn read_auth(&mut self, owner: &Value, location: &str, inherited: Option<&RequestAuth>, found: &mut Vec<String>) -> Option<RequestAuth> {
        let Some(auth) = owner.get("authentication").filter(|a| a.is_object()) else {
            return inherited.cloned();
        };

        let kind = text_or_empty(auth, "type");
        if kind.is_empty() || kind == "inherit" {
            return inherited.cloned();
        }
        if kind == "none" {
            return None;
        }
        if is_true(auth, "disabled") {
            self.issues.push(ImportIssue::new(ImportCategories::DROPPED_DATA, location, format!("disabled {kind} auth")));
            return None;
        }

        match kind.as_str() {
            auth_kinds::BEARER => {
                let prefix = text_or_empty(auth, "prefix");
                if !prefix.is_empty() && !prefix.eq_ignore_ascii_case("Bearer") {
                    self.issues.push(ImportIssue::new(
                        ImportCategories::UNSUPPORTED_AUTH,
                        location,
                        format!("bearer prefix \"{prefix}\" (the token is sent with Bearer)"),
                    ));
                }
                Some(RequestAuth { kind: auth_kinds::BEARER.to_string(), token: auth_value(auth, "token", found), ..RequestAuth::default() })
            }

            auth_kinds::BASIC => Some(RequestAuth {
                kind: auth_kinds::BASIC.to_string(),
                username: auth_value(auth, "username", found),
                password: auth_value(auth, "password", found),
                ..RequestAuth::default()
            }),

            "apikey" => Some(RequestAuth {
                kind: auth_kinds::API_KEY.to_string(),
                key_name: auth_value(auth, "key", found),
                key_value: auth_value(auth, "value", found),
                key_in_query: text_of(auth, "addTo").as_deref() == Some("queryParams"),
                ..RequestAuth::default()
            }),

            _ => {
                self.issues.push(ImportIssue::new(ImportCategories::UNSUPPORTED_AUTH, location, kind));
                None
            }
        }
    }

    fn read_variables(&mut self, environment: &Value, location: &str) -> Vec<KeyValue> {
        let mut found = Vec::new();
        let mut variables = Vec::new();
        if let Some(Value::Object(data)) = environment.get("data") {
            for (name, value) in data {
                variables.push(KeyValue::new(name.clone(), normalize(&variable_text(value), &mut found)));
            }
        }

        self.report_dynamic(&found, location);
        variables
    }

    fn note_description(&mut self, element: &Value) {
        if non_empty(text_of(element, "description")).is_some() {
            self.descriptions += 1;
        }
    }

    fn report_dynamic(&mut self, found: &[String], location: &str) {
        if found.is_empty() {
            return;
        }

        let mut distinct: Vec<&str> = Vec::new();
        for name in found {
            if !distinct.contains(&name.as_str()) {
                distinct.push(name);
            }
        }
        self.issues.push(ImportIssue::new(ImportCategories::DYNAMIC_VARIABLES, location, distinct.join(", ")));
    }
}

/// Headers from a folder or workspace apply to requests inside it, unless the request sets a header with the same name.
fn inherit(own: Vec<KeyValue>, inherited: &[KeyValue]) -> Vec<KeyValue> {
    let names: HashSet<String> = own.iter().filter(|h| h.enabled).map(|h| h.key.to_lowercase()).collect();
    let mut result = own;
    result.extend(inherited.iter().filter(|h| !names.contains(&h.key.to_lowercase())).cloned());
    result
}

fn read_headers(owner: &Value, found: &mut Vec<String>) -> Vec<KeyValue> {
    let mut headers = Vec::new();
    for header in elements(owner, "headers") {
        let name = text_or_empty(header, "name");
        if name.is_empty() {
            continue;
        }

        let key = normalize(&name, found);
        let value = normalize(&text_or_empty(header, "value"), found);
        headers.push(KeyValue { enabled: !is_true(header, "disabled"), ..KeyValue::new(key, value) });
    }

    headers
}

fn auth_value(auth: &Value, key: &str, found: &mut Vec<String>) -> String {
    normalize(&text_or_empty(auth, key), found)
}

/// Rewrites Insomnia placeholders to ours. Names that do not map are left as written and added to `found`
/// so the import report can list them.
fn normalize(text: &str, found: &mut Vec<String>) -> String {
    if text.is_empty() {
        return String::new();
    }

    let text = uuid_tag().replace_all(text, NoExpand("{{$guid}}")).into_owned();
    let text = unix_timestamp_tag().replace_all(&text, NoExpand("{{$timestamp}}")).into_owned();
    let text = simple_placeholder().replace_all(&text, |caps: &Captures| ["{{", &caps[1], "}}"].concat()).into_owned();

    for tag in tag_placeholder().find_iter(&text) {
        found.push(tag.as_str().to_string());
    }

    for caps in any_placeholder().captures_iter(&text) {
        let inner = caps[1].trim();
        if !simple_name().is_match(inner) && !builtins::is_postman_alias(inner) {
            found.push(inner.to_string());
        }
    }

    text
}

/// Escapes the text between placeholders and leaves the placeholders themselves as written.
fn escape_keeping_placeholders(text: &str) -> String {
    let mut out = String::new();
    let mut last = 0;
    for placeholder in placeholder_split().find_iter(text) {
        out.push_str(&escape_segment(&text[last..placeholder.start()]));
        out.push_str(placeholder.as_str());
        last = placeholder.end();
    }

    out.push_str(&escape_segment(&text[last..]));
    out
}

// A piece of text that starts with `{{` but is not a placeholder is kept as written, as the C# importer does.
fn escape_segment(segment: &str) -> String {
    if segment.starts_with("{{") { segment.to_string() } else { uri_escape(segment) }
}

// Same as .NET's Uri.EscapeDataString: unreserved characters stay, and every other UTF-8 byte is %-encoded.
fn uri_escape(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }

    out
}

fn add_content_type(headers: &mut Vec<KeyValue>, value: &str) {
    if !headers.iter().any(|h| h.enabled && h.key.eq_ignore_ascii_case("Content-Type")) {
        headers.push(KeyValue::new("Content-Type", value));
    }
}

/// Resources in an export, grouped by parent and ordered by Insomnia's sort key.
struct ResourceTree<'a> {
    all: Vec<&'a Value>,
    children: HashMap<String, Vec<&'a Value>>,
}

impl<'a> ResourceTree<'a> {
    fn new(resources: &'a [Value]) -> Self {
        let mut all = Vec::new();
        let mut children: HashMap<String, Vec<&'a Value>> = HashMap::new();
        for resource in resources {
            if !resource.is_object() {
                continue;
            }

            all.push(resource);
            children.entry(text_or_empty(resource, "parentId")).or_default().push(resource);
        }

        for list in children.values_mut() {
            list.sort_by(|a, b| sort_key(a).partial_cmp(&sort_key(b)).unwrap_or(std::cmp::Ordering::Equal));
        }

        Self { all, children }
    }

    fn of_type(&self, kind: &str) -> Vec<&'a Value> {
        let mut found: Vec<&'a Value> = self.all.iter().copied().filter(|r| text_or_empty(r, "_type") == kind).collect();
        found.sort_by(|a, b| sort_key(a).partial_cmp(&sort_key(b)).unwrap_or(std::cmp::Ordering::Equal));
        found
    }

    fn children(&self, parent_id: &str) -> &[&'a Value] {
        match self.children.get(parent_id) {
            Some(list) => list.as_slice(),
            None => &[],
        }
    }
}

fn sort_key(resource: &Value) -> f64 {
    resource.get("metaSortKey").and_then(Value::as_f64).unwrap_or(0.0)
}

fn variable_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        // Numbers, booleans, and the objects or arrays Insomnia allows as values.
        other => other.to_string(),
    }
}

fn count(count: usize, noun: &str) -> String {
    if count == 1 { format!("1 {noun}") } else { format!("{count} {noun}s") }
}

/// The array under `name`, or nothing when it is missing or not an array.
fn elements<'a>(element: &'a Value, name: &str) -> impl Iterator<Item = &'a Value> {
    element.get(name).and_then(Value::as_array).into_iter().flatten()
}

fn is_true(element: &Value, name: &str) -> bool {
    element.get(name).and_then(Value::as_bool) == Some(true)
}

fn text_of(element: &Value, name: &str) -> Option<String> {
    element.get(name).and_then(as_text)
}

fn text_or_empty(element: &Value, name: &str) -> String {
    text_of(element, name).unwrap_or_default()
}

fn non_empty(text: Option<String>) -> Option<String> {
    text.filter(|t| !t.is_empty())
}

// Insomnia allows numbers and booleans where a string is usual, such as environment values.
fn as_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(_) | Value::Bool(_) => Some(value.to_string()),
        _ => None,
    }
}

// The file name without its folder or extension, used to name a workspace that has no name.
fn file_stem(file_name: &str) -> String {
    let base = file_name.rsplit(['/', '\\']).next().unwrap_or("");
    match base.rsplit_once('.') {
        Some((stem, _)) => stem.to_string(),
        None => base.to_string(),
    }
}

macro_rules! pattern {
    ($name:ident, $pattern:expr) => {
        fn $name() -> &'static Regex {
            static PATTERN: OnceLock<Regex> = OnceLock::new();
            PATTERN.get_or_init(|| Regex::new($pattern).expect("valid pattern"))
        }
    };
}

// {{ name }} and {{ _.name }} (Insomnia's environment reference) both mean a Ricochet variable.
pattern!(simple_placeholder, r"\{\{\s*(?:_\.)?([A-Za-z_][\w-]*(?:\.[\w-]+)*)\s*\}\}");
pattern!(simple_name, r"^[A-Za-z_][\w-]*(?:\.[\w-]+)*$");
pattern!(any_placeholder, r"\{\{([^{}]*)\}\}");
pattern!(tag_placeholder, r"(?s)\{%.*?%\}");
pattern!(uuid_tag, r"\{%\s*uuid\s+'v4'\s*%\}");
pattern!(unix_timestamp_tag, r"\{%\s*timestamp\s+'unix'\s*%\}");
pattern!(placeholder_split, r"\{\{[^{}]+\}\}");
