//! Changes the window makes to a Request before it is saved. Plain functions on the Core model, so tests reach them
//! without a window. A Secret header's value never goes into the Request: it goes to the secrets vault, through
//! [`set_secret_header`], and is filled in only for sending, through [`fill_secret_headers`] (ADR 0092).

use std::collections::BTreeMap;

use ricochet_core::assertions::{RequestAssertion, RequestCapture, ops, sources};
use ricochet_core::models::{ApiRequest, CollectionItem, KeyValue, RequestAuth, Workspace, auth_kinds};
use ricochet_core::secrets::{SecretValue, header_key};

/// The methods the method button steps through, in order.
pub const METHODS: [&str; 7] = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

/// The method after this one in [`METHODS`], wrapping round. A method not in the list goes to the first.
pub fn next_method(method: &str) -> &'static str {
    let index = METHODS.iter().position(|known| known.eq_ignore_ascii_case(method));
    METHODS[index.map_or(0, |index| (index + 1) % METHODS.len())]
}

/// A text field of a Request that the window edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Url,
    HeaderKey(usize),
    HeaderValue(usize),
    /// The username of Basic auth. Not a Secret.
    AuthUsername,
    /// The name of the parameter that an API key goes in. Not a Secret.
    AuthKeyName,
    /// The path a check reads from, such as a header name or a JSON path.
    CheckPath(usize),
    /// The text a check compares with.
    CheckExpected(usize),
    /// The Variable a capture saves its value to.
    CaptureName(usize),
    /// The path a capture reads from, such as a header name or a JSON path.
    CapturePath(usize),
}

/// The sources a check reads from, in the order its button steps through them.
pub const CHECK_SOURCES: [&str; 5] = [sources::STATUS, sources::TIME, sources::HEADER, sources::BODY, sources::JSON];

/// The comparisons a check makes, in the order its button steps through them.
pub const CHECK_OPS: [&str; 9] = [
    ops::EQUAL,
    ops::NOT_EQUAL,
    ops::CONTAINS,
    ops::NOT_CONTAINS,
    ops::EXISTS,
    ops::NOT_EXISTS,
    ops::LESS_THAN,
    ops::GREATER_THAN,
    ops::MATCHES,
];

/// The auth kinds the window offers, in the order the kind button steps through them. The signing kinds are not
/// offered, because they need Secrets.
pub const AUTH_KINDS: [&str; 4] = [auth_kinds::NONE, auth_kinds::BEARER, auth_kinds::BASIC, auth_kinds::API_KEY];

/// Renames the Request in the sidebar item with this id. The sidebar shows the item's name and the Request's name, so
/// both are set. Returns false, and changes nothing, when the item is not a Request or the name is the same.
pub fn rename_request(workspace: &mut Workspace, item_id: &str, name: &str) -> bool {
    for collection in workspace.collections.iter_mut() {
        if let Some(item) = find_item_mut(&mut collection.items, item_id) {
            let Some(request) = item.request.as_mut() else {
                return false;
            };
            if item.name == name && request.name == name {
                return false;
            }
            item.name = name.to_string();
            request.name = name.to_string();
            return true;
        }
    }
    false
}

fn find_item_mut<'a>(items: &'a mut [CollectionItem], item_id: &str) -> Option<&'a mut CollectionItem> {
    for item in items {
        if item.id == item_id {
            return Some(item);
        }
        if let Some(found) = find_item_mut(&mut item.children, item_id) {
            return Some(found);
        }
    }
    None
}

/// The Request held by the sidebar item with this id, for changing.
pub fn find_request_mut<'a>(workspace: &'a mut Workspace, item_id: &str) -> Option<&'a mut ApiRequest> {
    workspace.collections.iter_mut().find_map(|collection| find_in(&mut collection.items, item_id))
}

fn find_in<'a>(items: &'a mut [CollectionItem], item_id: &str) -> Option<&'a mut ApiRequest> {
    for item in items {
        if item.id == item_id && item.request.is_some() {
            return item.request.as_mut();
        }
        if let Some(found) = find_in(&mut item.children, item_id) {
            return Some(found);
        }
    }
    None
}

/// Sets a field's text. Returns true when the Request changed. A header that does not exist, and the value of a
/// Secret header, are left alone.
pub fn set_field(request: &mut ApiRequest, field: Field, text: &str) -> bool {
    let target = match field {
        Field::Url => &mut request.url,
        Field::HeaderKey(index) => match request.headers.get_mut(index) {
            Some(header) => &mut header.key,
            None => return false,
        },
        Field::HeaderValue(index) => match request.headers.get_mut(index) {
            Some(header) if !header.is_secret => &mut header.value,
            _ => return false,
        },
        Field::AuthUsername => &mut request.auth.get_or_insert_with(RequestAuth::default).username,
        Field::AuthKeyName => &mut request.auth.get_or_insert_with(RequestAuth::default).key_name,
        Field::CheckPath(index) => match request.assertions.as_mut().and_then(|checks| checks.get_mut(index)) {
            Some(check) => &mut check.path,
            None => return false,
        },
        Field::CheckExpected(index) => match request.assertions.as_mut().and_then(|checks| checks.get_mut(index)) {
            Some(check) => &mut check.expected,
            None => return false,
        },
        Field::CaptureName(index) => match request.captures.as_mut().and_then(|captures| captures.get_mut(index)) {
            Some(capture) => &mut capture.variable,
            None => return false,
        },
        Field::CapturePath(index) => match request.captures.as_mut().and_then(|captures| captures.get_mut(index)) {
            Some(capture) => &mut capture.path,
            None => return false,
        },
    };
    if target == text {
        return false;
    }
    *target = text.to_string();
    true
}

/// Moves the Request to the next method. Always a change.
pub fn cycle_method(request: &mut ApiRequest) {
    request.method = next_method(&request.method).to_string();
}

/// Adds an empty, enabled header at the end and returns its index.
pub fn add_header(request: &mut ApiRequest) -> usize {
    request.headers.push(KeyValue::default());
    request.headers.len() - 1
}

/// Removes a header. Returns false when there is none at that index.
pub fn remove_header(request: &mut ApiRequest, index: usize) -> bool {
    if index >= request.headers.len() {
        return false;
    }
    request.headers.remove(index);
    true
}

/// Turns a header on or off. Returns false when there is none at that index.
pub fn toggle_header(request: &mut ApiRequest, index: usize) -> bool {
    match request.headers.get_mut(index) {
        Some(header) => {
            header.enabled = !header.enabled;
            true
        }
        None => false,
    }
}

/// Moves the auth kind to the next one in [`AUTH_KINDS`]. A kind outside the list, such as a signing kind, goes to
/// the first one. Settings of the other kinds are kept, so switching back loses nothing.
pub fn cycle_auth_kind(request: &mut ApiRequest) {
    let current = request.auth.as_ref().map_or(auth_kinds::NONE, |auth| auth.kind.as_str());
    let index = AUTH_KINDS.iter().position(|kind| *kind == current);
    let next = AUTH_KINDS[index.map_or(0, |index| (index + 1) % AUTH_KINDS.len())];
    request.auth.get_or_insert_with(RequestAuth::default).kind = next.to_string();
}

/// Turns the API key's query-string placement on or off. Creates the auth settings when there are none.
pub fn toggle_key_in_query(request: &mut ApiRequest) {
    let auth = request.auth.get_or_insert_with(RequestAuth::default);
    auth.key_in_query = !auth.key_in_query;
}

/// Moves a check's source to the next one in [`CHECK_SOURCES`]. Returns false when there is no check at that index.
pub fn cycle_check_source(request: &mut ApiRequest, index: usize) -> bool {
    cycle_in(request, index, &CHECK_SOURCES, |check| &mut check.source)
}

/// Moves a check's comparison to the next one in [`CHECK_OPS`]. Returns false when there is no check at that index.
pub fn cycle_check_op(request: &mut ApiRequest, index: usize) -> bool {
    cycle_in(request, index, &CHECK_OPS, |check| &mut check.op)
}

fn cycle_in(
    request: &mut ApiRequest,
    index: usize,
    choices: &[&str],
    field: impl FnOnce(&mut RequestAssertion) -> &mut String,
) -> bool {
    let Some(check) = request.assertions.as_mut().and_then(|checks| checks.get_mut(index)) else {
        return false;
    };
    let value = field(check);
    let position = choices.iter().position(|choice| *choice == value.as_str());
    *value = choices[position.map_or(0, |position| (position + 1) % choices.len())].to_string();
    true
}

/// Turns a check on or off. Returns false when there is no check at that index.
pub fn toggle_check(request: &mut ApiRequest, index: usize) -> bool {
    match request.assertions.as_mut().and_then(|checks| checks.get_mut(index)) {
        Some(check) => {
            check.enabled = !check.enabled;
            true
        }
        None => false,
    }
}

/// Adds a check that compares the status with nothing yet, and returns its index.
pub fn add_check(request: &mut ApiRequest) -> usize {
    let checks = request.assertions.get_or_insert_with(Vec::new);
    checks.push(RequestAssertion::default());
    checks.len() - 1
}

/// Removes a check. The list goes back to none when it is empty, so the Request's file adds nothing.
/// Returns false when there is no check at that index.
pub fn remove_check(request: &mut ApiRequest, index: usize) -> bool {
    let Some(checks) = request.assertions.as_mut() else {
        return false;
    };
    if index >= checks.len() {
        return false;
    }
    checks.remove(index);
    if checks.is_empty() {
        request.assertions = None;
    }
    true
}

/// The sources a capture reads from, in the order its button steps through them. Same as the C# editor offers.
pub const CAPTURE_SOURCES: [&str; 3] = [sources::JSON, sources::HEADER, sources::BODY];

/// Whether a capture from this source needs a path. A body capture has none, so its path is kept empty.
pub fn capture_needs_path(source: &str) -> bool {
    matches!(source, sources::HEADER | sources::JSON)
}

/// Adds a capture that reads a JSON value with nothing set yet, and returns its index.
pub fn add_capture(request: &mut ApiRequest) -> usize {
    let captures = request.captures.get_or_insert_with(Vec::new);
    captures.push(RequestCapture::default());
    captures.len() - 1
}

/// Removes a capture. The list goes back to none when it is empty, so the Request's file adds nothing.
/// Returns false when there is no capture at that index.
pub fn remove_capture(request: &mut ApiRequest, index: usize) -> bool {
    let Some(captures) = request.captures.as_mut() else {
        return false;
    };
    if index >= captures.len() {
        return false;
    }
    captures.remove(index);
    if captures.is_empty() {
        request.captures = None;
    }
    true
}

/// Turns a capture on or off. Returns false when there is no capture at that index.
pub fn toggle_capture(request: &mut ApiRequest, index: usize) -> bool {
    match request.captures.as_mut().and_then(|captures| captures.get_mut(index)) {
        Some(capture) => {
            capture.enabled = !capture.enabled;
            true
        }
        None => false,
    }
}

/// Moves a capture's source to the next one in [`CAPTURE_SOURCES`]. A source outside the list goes to the first one.
/// When the new source takes no path, the path is cleared, so the file never holds a path that does nothing.
/// Returns false when there is no capture at that index.
pub fn cycle_capture_source(request: &mut ApiRequest, index: usize) -> bool {
    let Some(capture) = request.captures.as_mut().and_then(|captures| captures.get_mut(index)) else {
        return false;
    };
    let position = CAPTURE_SOURCES.iter().position(|choice| *choice == capture.source.as_str());
    capture.source = CAPTURE_SOURCES[position.map_or(0, |position| (position + 1) % CAPTURE_SOURCES.len())].to_string();
    if !capture_needs_path(&capture.source) {
        capture.path.clear();
    }
    true
}

/// Stores a Secret typed in the window. Empty text removes it. Returns true when the stored value changed.
pub fn set_secret(secrets: &mut std::collections::BTreeMap<String, ricochet_core::secrets::SecretValue>, key: &str, text: &str) -> bool {
    if text.is_empty() {
        return secrets.remove(key).is_some();
    }
    if secrets.get(key).is_some_and(|value| value.expose() == text) {
        return false;
    }
    secrets.insert(key.to_string(), ricochet_core::secrets::SecretValue::new(text));
    true
}

/// Stores a value typed into a Secret header. The value goes to the vault under the header's key, and the Request
/// keeps an empty value, so the Workspace file never holds it. Returns true when the vault or the Request changed.
/// A header that is not a Secret, or no header at that index, is left alone and returns false.
pub fn set_secret_header(request: &mut ApiRequest, secrets: &mut BTreeMap<String, SecretValue>, index: usize, text: &str) -> bool {
    let Some(header) = request.headers.get_mut(index) else {
        return false;
    };
    if !header.is_secret {
        return false;
    }
    let key = header_key(&request.id, &header.key);
    let cleared = !header.value.is_empty();
    header.value.clear();
    let stored = set_secret(secrets, &key, text);
    cleared || stored
}

/// Changes a header's name. When the header is a Secret, its vault entry moves to the new name, so the value stays with
/// the header and is never sent under a name it was not typed for. Returns true when the name changed.
pub fn set_header_name(request: &mut ApiRequest, secrets: &mut BTreeMap<String, SecretValue>, index: usize, name: &str) -> bool {
    let Some(header) = request.headers.get(index) else {
        return false;
    };
    let old_key = header.is_secret.then(|| header_key(&request.id, &header.key));
    if !set_field(request, Field::HeaderKey(index), name) {
        return false;
    }
    if let Some(old_key) = old_key {
        if let Some(value) = secrets.remove(&old_key) {
            secrets.insert(header_key(&request.id, name), value);
        }
    }
    true
}

/// Removes a header, and forgets its Secret when it is one. Returns false when there is no header at that index.
pub fn remove_header_and_secret(request: &mut ApiRequest, secrets: &mut BTreeMap<String, SecretValue>, index: usize) -> bool {
    let Some(header) = request.headers.get(index) else {
        return false;
    };
    if header.is_secret {
        secrets.remove(&header_key(&request.id, &header.key));
    }
    remove_header(request, index)
}

/// Fills the value of each Secret header in `sent` from the vault, for sending. `stored` is the Request as the window
/// holds it, before Variables are applied, so each Secret is looked up under the name it was saved with. `sent` has the
/// same headers in the same order. Returns true when an enabled Secret header has no stored value.
pub fn fill_secret_headers(sent: &mut ApiRequest, stored: &ApiRequest, lookup: impl Fn(&str) -> Option<String>) -> bool {
    let mut missing = false;
    for (index, header) in stored.headers.iter().enumerate() {
        if !header.is_secret {
            continue;
        }
        let value = lookup(&header_key(&stored.id, &header.key));
        if header.enabled && value.is_none() {
            missing = true;
        }
        if let Some(sent_header) = sent.headers.get_mut(index) {
            sent_header.value = value.unwrap_or_default();
        }
    }
    missing
}

/// Which text of a variable row the window edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableField {
    Name,
    Value,
}

/// The Variables of the Collection or Folder with this id, for reading. None for a Request or an unknown id.
pub fn variables_of<'a>(workspace: &'a Workspace, item_id: &str) -> Option<&'a [KeyValue]> {
    if let Some(collection) = workspace.collections.iter().find(|collection| collection.id == item_id) {
        return Some(&collection.variables);
    }
    workspace
        .collections
        .iter()
        .find_map(|collection| folder_variables(&collection.items, item_id))
        .or_else(|| workspace.environments.iter().find(|environment| environment.id == item_id).map(|environment| environment.variables.as_slice()))
}

fn folder_variables<'a>(items: &'a [CollectionItem], item_id: &str) -> Option<&'a [KeyValue]> {
    for item in items {
        if item.id == item_id {
            return if item.request.is_none() { Some(&item.variables) } else { None };
        }
        if let Some(found) = folder_variables(&item.children, item_id) {
            return Some(found);
        }
    }
    None
}

/// The Variables of the Collection or Folder with this id, for changing. None for a Request or an unknown id.
pub fn variables_mut<'a>(workspace: &'a mut Workspace, item_id: &str) -> Option<&'a mut Vec<KeyValue>> {
    if let Some(index) = workspace.environments.iter().position(|environment| environment.id == item_id) {
        return Some(&mut workspace.environments[index].variables);
    }
    if let Some(index) = workspace.collections.iter().position(|collection| collection.id == item_id) {
        return Some(&mut workspace.collections[index].variables);
    }
    workspace.collections.iter_mut().find_map(|collection| folder_variables_mut(&mut collection.items, item_id))
}

fn folder_variables_mut<'a>(items: &'a mut [CollectionItem], item_id: &str) -> Option<&'a mut Vec<KeyValue>> {
    for item in items {
        if item.id == item_id {
            return if item.request.is_none() { Some(&mut item.variables) } else { None };
        }
        if let Some(found) = folder_variables_mut(&mut item.children, item_id) {
            return Some(found);
        }
    }
    None
}

/// Sets the name or value of the variable at `index`. Returns true when it changed. A row that does not exist is left alone.
pub fn set_variable_text(variables: &mut [KeyValue], index: usize, field: VariableField, text: &str) -> bool {
    let Some(variable) = variables.get_mut(index) else {
        return false;
    };
    let target = match field {
        VariableField::Name => &mut variable.key,
        VariableField::Value => &mut variable.value,
    };
    if target == text {
        return false;
    }
    *target = text.to_string();
    true
}

/// Adds an enabled variable with no name or value, and returns its index.
pub fn add_variable(variables: &mut Vec<KeyValue>) -> usize {
    variables.push(KeyValue { key: String::new(), value: String::new(), enabled: true, ..Default::default() });
    variables.len() - 1
}

/// Removes the variable at `index`. Returns false when there is none there.
pub fn remove_variable(variables: &mut Vec<KeyValue>, index: usize) -> bool {
    if index < variables.len() {
        variables.remove(index);
        return true;
    }
    false
}

/// Turns the variable at `index` on or off. Returns false when there is none there.
pub fn toggle_variable(variables: &mut [KeyValue], index: usize) -> bool {
    match variables.get_mut(index) {
        Some(variable) => {
            variable.enabled = !variable.enabled;
            true
        }
        None => false,
    }
}

/// Adds an Environment with this name, made unique among the Environments, and returns its id.
pub fn new_environment(workspace: &mut Workspace, name: &str) -> String {
    let name = crate::tree_edit::unique_name(name, workspace.environments.iter().map(|environment| environment.name.as_str()));
    let environment = ricochet_core::models::ApiEnvironment { name, ..Default::default() };
    let id = environment.id.clone();
    workspace.environments.push(environment);
    id
}

/// Renames the Environment with this id. Returns false, and changes nothing, when there is none or the name is the same.
pub fn rename_environment(workspace: &mut Workspace, id: &str, name: &str) -> bool {
    match workspace.environments.iter_mut().find(|environment| environment.id == id) {
        Some(environment) if environment.name != name => {
            environment.name = name.to_string();
            true
        }
        _ => false,
    }
}

/// Removes the Environment with this id. Returns false when there is none.
pub fn delete_environment(workspace: &mut Workspace, id: &str) -> bool {
    let before = workspace.environments.len();
    workspace.environments.retain(|environment| environment.id != id);
    workspace.environments.len() != before
}

/// The id of the Collection that holds the item with this id, or is that Collection. None when there is none.
pub fn collection_id_of(workspace: &Workspace, item_id: &str) -> Option<String> {
    workspace
        .collections
        .iter()
        .find(|collection| collection.id == item_id || contains_item(&collection.items, item_id))
        .map(|collection| collection.id.clone())
}

fn contains_item(items: &[CollectionItem], item_id: &str) -> bool {
    items.iter().any(|item| item.id == item_id || contains_item(&item.children, item_id))
}

/// The Variables of the Collection with this id, as they are by default. Empty when there is no such Collection.
pub fn collection_variables(workspace: &Workspace, collection_id: &str) -> Vec<KeyValue> {
    workspace.collections.iter().find(|collection| collection.id == collection_id).map(|collection| collection.variables.clone()).unwrap_or_default()
}

/// Sets the Environment's value for one Collection variable. An empty value removes the Environment's entry, so the
/// default applies again. Returns true when the Environment changed.
pub fn set_override(workspace: &mut Workspace, environment_id: &str, key: &str, value: &str) -> bool {
    let Some(environment) = workspace.environments.iter_mut().find(|environment| environment.id == environment_id) else {
        return false;
    };
    let position = environment.variables.iter().position(|variable| variable.key == key);
    match (position, value.is_empty()) {
        (Some(index), true) => {
            environment.variables.remove(index);
            true
        }
        (Some(index), false) => {
            if environment.variables[index].value == value {
                return false;
            }
            environment.variables[index].value = value.to_string();
            true
        }
        (None, true) => false,
        (None, false) => {
            environment.variables.push(KeyValue::new(key, value));
            true
        }
    }
}

/// The tags of the Folder with this id. Empty for a Request, a Collection, or an unknown id.
pub fn folder_tags_of(workspace: &Workspace, item_id: &str) -> Vec<String> {
    workspace.collections.iter().find_map(|collection| folder_tags_in(&collection.items, item_id)).unwrap_or_default()
}

fn folder_tags_in(items: &[CollectionItem], item_id: &str) -> Option<Vec<String>> {
    for item in items {
        if item.id == item_id {
            return if item.request.is_none() { Some(item.tags.clone().unwrap_or_default()) } else { None };
        }
        if let Some(found) = folder_tags_in(&item.children, item_id) {
            return Some(found);
        }
    }
    None
}

/// Sets the tags of the Folder with this id from comma separated text, as the Folder editor types them. Returns false,
/// and changes nothing, when there is no such Folder or the tags are the same.
pub fn set_folder_tags(workspace: &mut Workspace, item_id: &str, text: &str) -> bool {
    let tags = ricochet_core::folder_tags::normalize(Some(ricochet_core::folder_tags::parse(text)));
    for collection in workspace.collections.iter_mut() {
        if let Some(item) = folder_mut(&mut collection.items, item_id) {
            if item.tags == tags {
                return false;
            }
            item.tags = tags;
            return true;
        }
    }
    false
}

fn folder_mut<'a>(items: &'a mut [CollectionItem], item_id: &str) -> Option<&'a mut CollectionItem> {
    for item in items {
        if item.id == item_id {
            return if item.request.is_none() { Some(item) } else { None };
        }
        if let Some(found) = folder_mut(&mut item.children, item_id) {
            return Some(found);
        }
    }
    None
}
