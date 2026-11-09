//! Port of the parsing half of `src/Ricochet.Core/OpenApi.cs` (ADR 0014). Turns an OpenAPI 3 description in
//! JSON into a Collection with one Request per operation, and reports what an update would change.
//!
//! Nothing here touches the network. Fetching a description, which only happens when the user asks
//! (ADR 0005), is not ported yet, and neither are YAML and Swagger 2.0.

use std::collections::{HashMap, HashSet};
use std::fmt;

use serde_json::{Map, Value, json};

use crate::models::{ApiRequest, Collection, CollectionItem, KeyValue, RequestAuth, auth_kinds};

const BASE_URL_VARIABLE: &str = "baseUrl";
/// How far `$ref` chains and nested schemas are followed before giving up, as in the C# file.
const MAX_DEPTH: usize = 8;
const HTTP_METHODS: [&str; 8] = ["get", "put", "post", "delete", "options", "head", "patch", "trace"];
/// Stands in for a member that is missing, so lookups can return a reference.
static ABSENT: Value = Value::Null;

/// A description that could not be read. The message is suitable for showing to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenApiError {
    /// The text is not JSON. YAML is not supported yet.
    NotJson,
    /// The JSON is not an OpenAPI 3 description.
    NotOpenApi3,
}

impl OpenApiError {
    pub fn message(&self) -> &'static str {
        match self {
            Self::NotJson => "This is not JSON. Ricochet reads OpenAPI 3 in JSON, and YAML is not supported yet.",
            Self::NotOpenApi3 => "This is not an OpenAPI 3 description.",
        }
    }
}

impl fmt::Display for OpenApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for OpenApiError {}

/// What an update did, listed before the user applies it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenApiSummary {
    pub is_new: bool,
    pub added: Vec<String>,
    pub changed: Vec<String>,
    pub removed: Vec<String>,
    pub notes: Vec<String>,
}

impl OpenApiSummary {
    pub fn to_report_text(&self) -> String {
        let headline = if self.is_new {
            format!("New collection with {} requests.", self.added.len())
        } else {
            format!(
                "{} added, {} changed, {} no longer in the description.",
                self.added.len(),
                self.changed.len(),
                self.removed.len()
            )
        };
        let mut lines = vec![headline];
        add_section(&mut lines, "Added", &self.added);
        add_section(&mut lines, "Changed", &self.changed);
        add_section(&mut lines, "No longer in the description (kept, so delete them by hand if you want)", &self.removed);
        add_section(&mut lines, "Not imported", &self.notes);
        lines.join("\n")
    }
}

fn add_section(lines: &mut Vec<String>, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }

    lines.push(String::new());
    lines.push(format!("{title}:"));
    lines.extend(items.iter().map(|item| format!("  {item}")));
}

/// The Collection an OpenAPI description produces, with the list of what changed.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenApiResult {
    pub collection: Collection,
    pub summary: OpenApiSummary,
}

/// Builds a Collection from an OpenAPI 3 description in JSON (ADR 0014).
///
/// `existing` is the Collection an earlier update made from the same address, or `None` to create one.
/// `spec_url` is the address the text came from. It is recorded on the Collection and never fetched.
pub fn build(json: &str, spec_url: &str, existing: Option<Collection>) -> Result<OpenApiResult, OpenApiError> {
    let document: Value = serde_json::from_str(json).map_err(|_| OpenApiError::NotJson)?;
    let is_openapi_3 = document
        .as_object()
        .and_then(|root| root.get("openapi"))
        .and_then(Value::as_str)
        .is_some_and(|version| version.starts_with("3."));
    if !is_openapi_3 {
        return Err(OpenApiError::NotOpenApi3);
    }

    Ok(build_from(&document, spec_url, existing))
}

fn build_from<'a>(root: &'a Value, spec_url: &str, existing: Option<Collection>) -> OpenApiResult {
    let mut reader = Reader::new(root);
    let base_url = server_url(&mut reader);
    let components = reader.get(root, "components");
    let schemes = reader.get(components, "securitySchemes");
    let global_security = root.get("security").unwrap_or(&ABSENT);

    let groups = tag_groups(&mut reader);
    reader.tag_groups = groups;
    let operations = read_operations(&mut reader, schemes, global_security);

    let is_new = existing.is_none();
    let mut collection = existing.unwrap_or_else(|| Collection { name: title(root, spec_url), ..Collection::default() });

    let mut variables = collection.variables.clone();
    add_variable(&mut variables, KeyValue::new(BASE_URL_VARIABLE, base_url));
    for operation in &operations {
        for variable in &operation.variables {
            add_variable(&mut variables, variable.clone());
        }
    }

    // Requests the description still has are updated in place, and the ones it no longer has are reported.
    let mut diff = Diff::new(&operations);
    let mut items = diff.walk(&collection.items);

    // New operations go into the folders they belong to, created when needed.
    let mut added = Vec::new();
    for operation in operations.iter().filter(|operation| !diff.matched.contains(&operation.key)) {
        let item = CollectionItem {
            name: operation.name.clone(),
            request: Some(operation.request.clone()),
            ..CollectionItem::default()
        };
        place(&mut items, &operation.folders, item);

        let mut path = operation.folders.clone();
        path.push(operation.name.clone());
        added.push(path.join(" / "));
    }
    let Diff { changed, removed, .. } = diff;

    let mut notes = dedupe(std::mem::take(&mut reader.notes));
    match reader.operations_with_unadded_parameters {
        0 => {}
        1 => notes.push("1 operation has query or header parameters, which are not added yet. Add them by hand.".to_string()),
        count => notes.push(format!(
            "{count} operations have query or header parameters, which are not added yet. Add them by hand."
        )),
    }

    collection.items = items;
    collection.variables = variables;
    collection.spec_url = Some(spec_url.to_string());

    OpenApiResult {
        collection,
        summary: OpenApiSummary { is_new, added, changed, removed, notes },
    }
}

/// Matches what the description has to what the Collection already holds, by operation key.
struct Diff<'o> {
    by_key: HashMap<&'o str, &'o Operation>,
    matched: HashSet<String>,
    changed: Vec<String>,
    removed: Vec<String>,
}

impl<'o> Diff<'o> {
    fn new(operations: &'o [Operation]) -> Self {
        Self {
            by_key: operations.iter().map(|operation| (operation.key.as_str(), operation)).collect(),
            matched: HashSet::new(),
            changed: Vec::new(),
            removed: Vec::new(),
        }
    }

    fn walk(&mut self, items: &[CollectionItem]) -> Vec<CollectionItem> {
        let mut result = Vec::new();
        for item in items {
            let Some(request) = &item.request else {
                let mut folder = item.clone();
                folder.children = self.walk(&item.children);
                result.push(folder);
                continue;
            };

            let Some(key) = request.spec_key.as_deref() else {
                result.push(item.clone());
                continue;
            };

            let Some(operation) = self.by_key.get(key).copied() else {
                self.removed.push(item.name.clone());
                result.push(item.clone());
                continue;
            };

            self.matched.insert(key.to_string());
            let merged = merge(request, &operation.request);
            if merged != *request || item.name != operation.name {
                self.changed.push(operation.name.clone());
            }

            result.push(CollectionItem { name: operation.name.clone(), request: Some(merged), ..item.clone() });
        }

        result
    }
}

/// Puts an item inside the chain of folders it belongs to, creating folders as needed.
fn place(level: &mut Vec<CollectionItem>, folders: &[String], item: CollectionItem) {
    let Some((first, rest)) = folders.split_first() else {
        level.push(item);
        return;
    };

    let index = match level.iter().position(|candidate| candidate.request.is_none() && &candidate.name == first) {
        Some(index) => index,
        None => {
            level.push(CollectionItem { name: first.clone(), ..CollectionItem::default() });
            level.len() - 1
        }
    };

    place(&mut level[index].children, rest, item);
}

/// Reads `x-tagGroups`, used by some documentation tools to group tags, and maps each tag to its group.
fn tag_groups(reader: &mut Reader<'_>) -> HashMap<String, String> {
    let mut groups = HashMap::new();
    let root = reader.root;
    let list = reader.get(root, "x-tagGroups");
    let Some(list) = list.as_array() else {
        return groups;
    };

    for group in list {
        let name = text(group, "name");
        let tags = reader.get(group, "tags");
        let Some(tags) = tags.as_array() else {
            continue;
        };
        if name.is_empty() {
            continue;
        }

        for tag in tags.iter().filter_map(Value::as_str) {
            groups.entry(tag.to_string()).or_insert_with(|| name.to_string());
        }
    }

    groups
}

/// The first real word of a path, such as "account" for /v1/account. None when the path has none.
fn resource_name(path: &str) -> Option<&str> {
    for part in path.split('/').filter(|part| !part.is_empty()) {
        if part.starts_with('{') {
            return None;
        }

        let is_version = part.len() > 1 && part.starts_with('v') && part[1..].chars().all(|c| c.is_ascii_digit());
        if part == "api" || is_version {
            continue;
        }

        return Some(part);
    }

    None
}

fn read_operations<'a>(reader: &mut Reader<'a>, schemes: &'a Value, global_security: &'a Value) -> Vec<Operation> {
    let mut operations = Vec::new();
    let mut seen = HashSet::new();
    let root = reader.root;
    let paths = reader.get(root, "paths");
    let Some(paths) = paths.as_object() else {
        return operations;
    };

    for (path, value) in paths {
        let path_item = reader.resolve(value);
        for method in HTTP_METHODS {
            let source = reader.get(path_item, method);
            if !source.is_object() {
                continue;
            }

            let operation_id = text(source, "operationId");
            let key = if operation_id.is_empty() {
                format!("{} {path}", method.to_uppercase())
            } else {
                operation_id.to_string()
            };

            if !seen.insert(key.clone()) {
                reader.notes.push(format!("The operation {key} appears more than once, so only the first was imported."));
                continue;
            }

            operations.push(read_operation(reader, &key, method, path, path_item, source, schemes, global_security));
        }
    }

    operations
}

#[allow(clippy::too_many_arguments)]
fn read_operation<'a>(
    reader: &mut Reader<'a>,
    key: &str,
    method: &str,
    path: &str,
    path_item: &'a Value,
    source: &'a Value,
    schemes: &'a Value,
    global_security: &'a Value,
) -> Operation {
    let summary = text(source, "summary");
    let operation_id = text(source, "operationId");
    let name = if !summary.is_empty() {
        summary.to_string()
    } else if !operation_id.is_empty() {
        operation_id.to_string()
    } else {
        format!("{} {path}", method.to_uppercase())
    };

    let tags = reader.get(source, "tags");
    let tag = tags.as_array().and_then(|tags| tags.first()).and_then(Value::as_str);

    // Folders, outermost first: the tag group if there is one, then the tag. Without tags, the path's resource.
    let mut folders = Vec::new();
    if let Some(group) = tag.and_then(|tag| reader.tag_groups.get(tag)) {
        folders.push(group.clone());
    }
    if let Some(leaf) = tag.or_else(|| resource_name(path)) {
        folders.push(leaf.to_string());
    }

    if has_query_or_header_parameters(reader, source) {
        reader.operations_with_unadded_parameters += 1;
    }

    // Each path parameter gets a Variable, left empty for the user to fill in per Environment.
    let mut variables = Vec::new();
    for parameter in path_parameter_names(reader, path_item, source) {
        add_variable(&mut variables, KeyValue::new(parameter, ""));
    }

    let auth = read_auth(reader, source, schemes, global_security, &mut variables);
    let body = read_body(reader, source);

    let request = ApiRequest {
        name: name.clone(),
        method: method.to_uppercase(),
        // Path parameters become {{variables}}, so each one is listed as a placeholder until it has a value.
        url: format!("{}{}", placeholder(BASE_URL_VARIABLE), path.replace('{', "{{").replace('}', "}}")),
        auth,
        body: body.unwrap_or_default(),
        spec_key: Some(key.to_string()),
        ..ApiRequest::default()
    };

    Operation { key: key.to_string(), name, folders, request, variables }
}

/// The names of the path parameters an operation uses, from its own list and the path's shared list.
fn path_parameter_names<'a>(reader: &mut Reader<'a>, path_item: &'a Value, source: &'a Value) -> Vec<&'a str> {
    let mut names = Vec::new();
    for list in [reader.get(path_item, "parameters"), reader.get(source, "parameters")] {
        let Some(list) = list.as_array() else {
            continue;
        };

        for parameter in list {
            let resolved = reader.resolve(parameter);
            let name = text(resolved, "name");
            if text(resolved, "in") == "path" && !name.is_empty() {
                names.push(name);
            }
        }
    }

    names
}

fn has_query_or_header_parameters<'a>(reader: &mut Reader<'a>, source: &'a Value) -> bool {
    let parameters = reader.get(source, "parameters");
    let Some(parameters) = parameters.as_array() else {
        return false;
    };

    for parameter in parameters {
        let resolved = reader.resolve(parameter);
        if matches!(text(resolved, "in"), "query" | "header") {
            return true;
        }
    }

    false
}

/// The first security requirement decides the auth. Values are never copied: they become variables.
fn read_auth<'a>(
    reader: &mut Reader<'a>,
    source: &'a Value,
    schemes: &'a Value,
    global_security: &'a Value,
    variables: &mut Vec<KeyValue>,
) -> Option<RequestAuth> {
    let security = source.get("security").unwrap_or(global_security);
    let requirement = security.as_array()?.first()?;
    let (scheme_name, _) = requirement.as_object()?.iter().next()?;

    read_scheme(reader, scheme_name, schemes, variables)
}

fn read_scheme<'a>(
    reader: &mut Reader<'a>,
    scheme_name: &str,
    schemes: &'a Value,
    variables: &mut Vec<KeyValue>,
) -> Option<RequestAuth> {
    let scheme = reader.get(schemes, scheme_name);
    if !scheme.is_object() {
        reader.notes.push(format!(
            "The sign-in method {scheme_name} is not defined in the description, so requests have no auth for it."
        ));
        return None;
    }

    let variable = variable_name(scheme_name);
    let kind = text(scheme, "type");
    let http_scheme = text(scheme, "scheme");

    if kind == "http" && http_scheme.eq_ignore_ascii_case("bearer") {
        variables.push(secret_variable(&variable));
        return Some(RequestAuth {
            kind: auth_kinds::BEARER.to_string(),
            token: placeholder(&variable),
            ..RequestAuth::default()
        });
    }

    if kind == "http" && http_scheme.eq_ignore_ascii_case("basic") {
        let username = format!("{variable}Username");
        let password = format!("{variable}Password");
        variables.push(KeyValue::new(username.as_str(), ""));
        variables.push(secret_variable(&password));
        return Some(RequestAuth {
            kind: auth_kinds::BASIC.to_string(),
            username: placeholder(&username),
            password: placeholder(&password),
            ..RequestAuth::default()
        });
    }

    if kind == "apiKey" && matches!(text(scheme, "in"), "header" | "query") {
        variables.push(secret_variable(&variable));
        return Some(RequestAuth {
            kind: auth_kinds::API_KEY.to_string(),
            key_name: text(scheme, "name").to_string(),
            key_value: placeholder(&variable),
            key_in_query: text(scheme, "in") == "query",
            ..RequestAuth::default()
        });
    }

    reader.notes.push(format!(
        "The sign-in method {scheme_name} ({kind}) is not supported yet, so its requests have no auth. Set it by hand."
    ));
    None
}

/// A JSON body is filled from the description's example, or built from its schema. Other types are reported.
fn read_body<'a>(reader: &mut Reader<'a>, source: &'a Value) -> Option<String> {
    let request_body = reader.get(source, "requestBody");
    let content = reader.get(request_body, "content");
    let content = content.as_object()?;

    let prefix = "application/json";
    for (media_type, media) in content {
        if !media_type.get(..prefix.len()).is_some_and(|start| start.eq_ignore_ascii_case(prefix)) {
            continue;
        }

        let value = write_media_value(reader, media);
        return Some(serde_json::to_string_pretty(&value).expect("a JSON value always serializes"));
    }

    reader.notes.push("Some requests have a body that is not JSON, so those bodies were not filled in.".to_string());
    None
}

fn write_media_value<'a>(reader: &mut Reader<'a>, media: &'a Value) -> Value {
    if let Some(example) = media.get("example") {
        return example.clone();
    }

    let examples = reader.get(media, "examples");
    if let Some((_, first)) = examples.as_object().and_then(|examples| examples.iter().next()) {
        if let Some(value) = reader.get_opt(first, "value") {
            return value.clone();
        }
    }

    let schema = reader.get(media, "schema");
    write_sample(reader, schema, 0)
}

/// Writes a value of the schema's shape, using examples, defaults or placeholders.
fn write_sample<'a>(reader: &mut Reader<'a>, schema: &'a Value, depth: usize) -> Value {
    let schema = reader.resolve(schema);
    if !schema.is_object() || depth > MAX_DEPTH {
        return Value::Null;
    }

    if let Some(example) = schema.get("example") {
        return example.clone();
    }

    if let Some(fallback) = schema.get("default") {
        return fallback.clone();
    }

    let choices = reader.get(schema, "enum");
    if let Some(choice) = choices.as_array().and_then(|choices| choices.first()) {
        return choice.clone();
    }

    if let Some(option) = first_of(schema, "oneOf").or_else(|| first_of(schema, "anyOf")) {
        return write_sample(reader, option, depth + 1);
    }

    let mut kind = text(schema, "type");
    if kind.is_empty() && (schema.get("properties").is_some() || schema.get("allOf").is_some()) {
        kind = "object";
    }

    match kind {
        "object" => Value::Object(write_properties(reader, schema, depth)),
        "array" => match schema.get("items") {
            Some(items) => Value::Array(vec![write_sample(reader, items, depth + 1)]),
            None => Value::Array(Vec::new()),
        },
        "integer" | "number" => json!(0),
        "boolean" => json!(false),
        "string" => json!(""),
        _ => Value::Null,
    }
}

fn write_properties<'a>(reader: &mut Reader<'a>, schema: &'a Value, depth: usize) -> Map<String, Value> {
    let mut sources = vec![schema];
    if let Some(parts) = schema.get("allOf").and_then(Value::as_array) {
        sources.extend(parts.iter().map(|part| reader.resolve(part)));
    }

    let mut object = Map::new();
    for source in sources {
        let properties = reader.get(source, "properties");
        let Some(properties) = properties.as_object() else {
            continue;
        };

        for (name, property) in properties {
            let sample = write_sample(reader, property, depth + 1);
            object.insert(name.clone(), sample);
        }
    }

    object
}

fn first_of<'a>(schema: &'a Value, keyword: &str) -> Option<&'a Value> {
    schema.get(keyword)?.as_array()?.first()
}

/// Keeps what the user has added or changed, and only refreshes what the description owns.
fn merge(existing: &ApiRequest, from_description: &ApiRequest) -> ApiRequest {
    // Body, headers, assertions and captures belong to the user once they exist, so they are left alone.
    ApiRequest {
        name: from_description.name.clone(),
        method: from_description.method.clone(),
        url: from_description.url.clone(),
        auth: from_description.auth.clone().or_else(|| existing.auth.clone()),
        ..existing.clone()
    }
}

fn server_url(reader: &mut Reader<'_>) -> String {
    let root = reader.root;
    let servers = reader.get(root, "servers");
    let Some(first) = servers.as_array().and_then(|servers| servers.first()) else {
        reader.notes.push("The description lists no server, so set baseUrl by hand.".to_string());
        return String::new();
    };

    let url = text(first, "url").trim_end_matches('/');
    if !url.contains("://") {
        reader.notes.push("The server address is relative, so set baseUrl to the full address by hand.".to_string());
    } else if url.contains('{') {
        reader.notes.push("The server address has variables, so set baseUrl by hand.".to_string());
    }

    url.to_string()
}

/// The title of the description, or the host of its address when it has none.
fn title(root: &Value, spec_url: &str) -> String {
    let title = text(root.get("info").unwrap_or(&ABSENT), "title").trim();
    if !title.is_empty() {
        return title.to_string();
    }

    host_of(spec_url).unwrap_or_else(|| "API".to_string())
}

/// The host of an absolute http or https address, lowercased. Brackets are kept for IPv6, and userinfo and port are dropped.
fn host_of(address: &str) -> Option<String> {
    let (_, rest) = address.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host_and_port = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    let host = match host_and_port.find(']') {
        Some(end) => &host_and_port[..=end],
        None => host_and_port.split(':').next().unwrap_or(""),
    };

    let host = host.to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// Adds a Variable only when it is not there yet, so values the user typed are never overwritten.
fn add_variable(variables: &mut Vec<KeyValue>, variable: KeyValue) {
    if !variables.iter().any(|existing| existing.key == variable.key) {
        variables.push(variable);
    }
}

fn secret_variable(key: &str) -> KeyValue {
    KeyValue { is_secret: true, ..KeyValue::new(key, "") }
}

fn placeholder(name: &str) -> String {
    ["{{", name, "}}"].concat()
}

/// A scheme name made safe for a Variable name. Each UTF-16 unit that is not a letter or digit becomes `_`, as in C#.
fn variable_name(scheme_name: &str) -> String {
    let mut name = String::new();
    for c in scheme_name.chars() {
        if c.len_utf16() > 1 {
            name.push_str("__");
        } else if c.is_alphanumeric() {
            name.push(c);
        } else {
            name.push('_');
        }
    }

    name
}

fn dedupe(notes: Vec<String>) -> Vec<String> {
    let mut unique: Vec<String> = Vec::new();
    for note in notes {
        if !unique.contains(&note) {
            unique.push(note);
        }
    }

    unique
}

/// The string at `name`, or an empty string when the member is missing or not a string.
fn text<'v>(value: &'v Value, name: &str) -> &'v str {
    value.get(name).and_then(Value::as_str).unwrap_or("")
}

/// One operation read from the description, before it is matched to a Request already in the Collection.
struct Operation {
    key: String,
    name: String,
    folders: Vec<String>,
    request: ApiRequest,
    variables: Vec<KeyValue>,
}

/// Follows references inside the same document, and records the rest as not followed.
struct Reader<'a> {
    root: &'a Value,
    notes: Vec<String>,
    operations_with_unadded_parameters: usize,
    tag_groups: HashMap<String, String>,
}

impl<'a> Reader<'a> {
    fn new(root: &'a Value) -> Self {
        Self { root, notes: Vec::new(), operations_with_unadded_parameters: 0, tag_groups: HashMap::new() }
    }

    /// The member `name` of `value` with references followed. None when it is missing or cannot be followed.
    fn get_opt(&mut self, value: &'a Value, name: &str) -> Option<&'a Value> {
        let child = value.get(name)?;
        self.deref(child)
    }

    /// Like `get_opt`, but a missing member reads as an absent value rather than None.
    fn get(&mut self, value: &'a Value, name: &str) -> &'a Value {
        self.get_opt(value, name).unwrap_or(&ABSENT)
    }

    /// Follows `$ref` to the value it names, or yields an absent value when it cannot be followed.
    fn resolve(&mut self, value: &'a Value) -> &'a Value {
        self.deref(value).unwrap_or(&ABSENT)
    }

    fn deref(&mut self, value: &'a Value) -> Option<&'a Value> {
        let mut element = value;
        for _ in 0..MAX_DEPTH {
            let Some(reference) = element.get("$ref").and_then(Value::as_str) else {
                return Some(element);
            };

            let Some(path) = reference.strip_prefix("#/") else {
                self.notes.push(format!("The reference {reference} points outside this description, so it was not followed."));
                return None;
            };

            let mut current = self.root;
            for part in path.split('/') {
                let key = part.replace("~1", "/").replace("~0", "~");
                current = current.get(key.as_str())?;
            }

            element = current;
        }

        Some(element)
    }
}
