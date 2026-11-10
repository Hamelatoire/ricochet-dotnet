use pretty_assertions::assert_eq;
use ricochet_core::duplicate::{copy_name, duplicate};
use ricochet_core::models::{ApiRequest, Collection, CollectionItem, KeyValue, RequestAuth, auth_kinds};
use ricochet_core::assertions::{RequestAssertion, RequestCapture};

fn request(id: &str, name: &str, url: &str) -> CollectionItem {
    CollectionItem {
        id: id.into(),
        name: name.into(),
        request: Some(ApiRequest {
            id: format!("{id}-req"),
            name: name.into(),
            url: url.into(),
            ..ApiRequest::default()
        }),
        ..CollectionItem::default()
    }
}

fn request_at(id: &str, name: &str) -> CollectionItem {
    request(id, name, "https://example.test")
}

fn folder(id: &str, name: &str, children: Vec<CollectionItem>) -> CollectionItem {
    CollectionItem { id: id.into(), name: name.into(), children, ..CollectionItem::default() }
}

fn collection(items: Vec<CollectionItem>) -> Collection {
    Collection { name: "Shop".into(), items, ..Collection::default() }
}

fn names(items: &[CollectionItem]) -> Vec<&str> {
    items.iter().map(|i| i.name.as_str()).collect()
}

#[test]
fn copy_name_is_name_copy_then_numbered_when_taken() {
    assert_eq!(copy_name("Login", &["Other"]), "Login (copy)");
    assert_eq!(copy_name("Login", &["Login (copy)"]), "Login (copy 2)");
    assert_eq!(copy_name("Login", &["Login (copy)", "login (copy 2)"]), "Login (copy 3)");
}

#[test]
fn duplicating_a_request_puts_the_copy_straight_after_it_with_new_ids() {
    let original = collection(vec![request_at("a", "Get"), request_at("b", "Post")]);

    let result = duplicate(&original, "a").unwrap();

    assert_eq!(names(&result.collection.items), ["Get", "Get (copy)", "Post"]);

    let copy = &result.copy;
    assert_eq!(copy, &result.collection.items[1]);
    assert_ne!(copy.id, "a");
    let copy_request = copy.request.as_ref().unwrap();
    assert_ne!(copy_request.id, "a-req");
    assert_eq!(copy_request.name, "Get (copy)");
    assert_eq!(copy_request.url, "https://example.test");
}

#[test]
fn the_copy_keeps_everything_that_belongs_to_the_request() {
    let mut original = request_at("a", "Get");
    original.request = Some(ApiRequest {
        id: "a-req".into(),
        name: "Get".into(),
        url: "{{base}}/items".into(),
        headers: vec![KeyValue::new("X-Trace", "1")],
        body: "{}".into(),
        assertions: Some(vec![RequestAssertion { expected: "200".into(), ..RequestAssertion::default() }]),
        captures: Some(vec![RequestCapture { path: "id".into(), variable: "itemId".into(), ..RequestCapture::default() }]),
        auth: Some(RequestAuth { kind: auth_kinds::BEARER.into(), token: "{{token}}".into(), ..RequestAuth::default() }),
        ..ApiRequest::default()
    });

    let result = duplicate(&collection(vec![original]), "a").unwrap();
    let copy = result.copy.request.unwrap();

    assert_eq!(copy.url, "{{base}}/items");
    assert_eq!(copy.headers, vec![KeyValue::new("X-Trace", "1")]);
    assert_eq!(copy.body, "{}");
    assert_eq!(copy.assertions.unwrap()[0].expected, "200");
    assert_eq!(copy.captures.unwrap()[0].variable, "itemId");
    assert_eq!(copy.auth.unwrap().token, "{{token}}");
}

#[test]
fn duplicating_a_folder_copies_its_requests_with_new_ids_and_keeps_their_order() {
    let original = collection(vec![folder("f", "Smoke", vec![request_at("r1", "One"), request_at("r2", "Two")])]);

    let copy = duplicate(&original, "f").unwrap().copy;

    assert_eq!(copy.name, "Smoke (copy)");
    assert_eq!(names(&copy.children), ["One", "Two"]);
    assert!(copy.children.iter().all(|c| c.id != "r1" && c.id != "r2"));
    let mut ids: Vec<&str> = copy.children.iter().map(|c| c.id.as_str()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 2);

    let child_request_ids: Vec<&str> = copy.children.iter().map(|c| c.request.as_ref().unwrap().id.as_str()).collect();
    assert!(child_request_ids.iter().all(|id| *id != "r1-req" && *id != "r2-req"));
}

#[test]
fn duplicating_a_nested_request_inserts_the_copy_in_its_own_folder() {
    let original = collection(vec![
        folder("f", "Smoke", vec![request_at("r1", "One"), request_at("r2", "Two")]),
        request_at("x", "Other"),
    ]);

    let result = duplicate(&original, "r1").unwrap();

    assert_eq!(names(&result.collection.items[0].children), ["One", "One (copy)", "Two"]);
    assert_eq!(names(&result.collection.items), ["Smoke", "Other"]);
}

#[test]
fn names_are_unique_among_siblings_only() {
    let original = collection(vec![
        request_at("a", "Get"),
        request_at("b", "Get (copy)"),
        folder("f", "Box", vec![request_at("c", "Get")]),
    ]);

    let first = duplicate(&original, "a").unwrap();
    assert_eq!(first.copy.name, "Get (copy 2)");

    let inside = duplicate(&original, "c").unwrap();
    assert_eq!(inside.copy.name, "Get (copy)");
}

#[test]
fn a_copy_of_a_hidden_item_is_hidden_and_the_original_stays_hidden() {
    let mut original = collection(vec![folder("f", "Smoke", vec![request_at("r1", "One")])]);
    original.hidden_item_ids = Some(vec!["f".into(), "r1".into()]);

    let result = duplicate(&original, "f").unwrap();
    let hidden = result.collection.hidden_item_ids.unwrap();

    assert_eq!(hidden.iter().filter(|id| *id == "f" || *id == "r1").count(), 2);
    let copy_child = &result.copy.children[0];
    assert!(hidden.contains(&copy_child.id));
    assert!(hidden.contains(&result.copy.id));
    assert_eq!(hidden.len(), 4);
}

#[test]
fn a_visible_item_copy_leaves_hidden_ids_unchanged() {
    let mut original = collection(vec![request_at("a", "Get"), request_at("b", "Post")]);
    original.hidden_item_ids = Some(vec!["b".into()]);

    let result = duplicate(&original, "a").unwrap();

    assert_eq!(result.collection.hidden_item_ids, Some(vec!["b".to_string()]));
}

#[test]
fn unknown_id_returns_none() {
    let original = collection(vec![request_at("a", "Get")]);

    assert_eq!(duplicate(&original, "missing"), None);
}
