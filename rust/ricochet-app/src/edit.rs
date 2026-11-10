//! Changes the window makes to a Request before it is saved. Plain functions on the Core model, so tests reach them
//! without a window. Secret values are never edited here: a Secret header's value is left alone (ADR 0092 is not
//! built, so nothing in this app may put a Secret into a Workspace file).

use ricochet_core::models::{ApiRequest, CollectionItem, KeyValue, Workspace};

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
