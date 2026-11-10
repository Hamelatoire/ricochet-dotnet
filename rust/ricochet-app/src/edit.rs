//! Changes the window makes to a Request before it is saved. Plain functions on the Core model, so tests reach them
//! without a window. Secret values are never edited here: a Secret header's value is left alone (ADR 0092 is not
//! built, so nothing in this app may put a Secret into a Workspace file).

use ricochet_core::assertions::{RequestAssertion, ops, sources};
use ricochet_core::models::{ApiRequest, CollectionItem, KeyValue, RequestAuth, Workspace, auth_kinds};

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
