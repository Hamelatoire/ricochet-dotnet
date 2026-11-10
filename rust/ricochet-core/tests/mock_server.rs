use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

use pretty_assertions::assert_eq;
use ricochet_core::mock_server::*;
use ricochet_core::models::KeyValue;
use serde_json::{json, Value};

const SPEC: &str = r##"{
  "openapi": "3.0.3",
  "info": { "title": "Pets" },
  "servers": [ { "url": "https://api.example.test/v1" } ],
  "paths": {
    "/pets": {
      "get": { "responses": { "200": { "description": "ok", "content": { "application/json": {
        "example": [ { "id": 1, "name": "Rex" } ] } } } } },
      "post": { "responses": { "201": { "description": "made", "content": { "application/json": {
        "schema": { "$ref": "#/components/schemas/Pet" } } } } } }
    },
    "/pets/{petId}": {
      "get": { "responses": { "200": { "description": "one", "content": { "application/json": {
        "examples": { "rex": { "value": { "id": 1, "name": "Rex" } } } } } } } },
      "delete": { "responses": { "204": { "description": "gone" } } }
    },
    "/pets/search": {
      "get": { "responses": { "default": { "description": "anything", "content": { "text/plain": {
        "example": "found" } } } } }
    }
  },
  "components": {
    "schemas": {
      "Pet": {
        "type": "object",
        "properties": {
          "name": { "type": "string" },
          "age": { "type": "integer" },
          "active": { "type": "boolean" },
          "tags": { "type": "array", "items": { "type": "string" } },
          "owner": { "$ref": "#/components/schemas/Owner" },
          "status": { "type": "string", "enum": ["sold", "available"] },
          "born": { "type": "string", "format": "date" }
        }
      },
      "Owner": {
        "allOf": [
          { "$ref": "#/components/schemas/Named" },
          { "type": "object", "properties": { "email": { "type": "string", "format": "email" } } }
        ]
      },
      "Named": { "type": "object", "properties": { "fullName": { "type": "string" } } }
    }
  }
}"##;

fn body(reply: &MockReply) -> String {
    String::from_utf8(reply.body.clone()).unwrap()
}

fn route(method: &str, path: &str, status: u16, text: &str) -> MockRoute {
    MockRoute::new(
        method,
        path,
        MockReply {
            status_code: status,
            headers: Vec::new(),
            body: text.as_bytes().to_vec(),
            media_type: Some("text/plain".to_string()),
        },
    )
}

fn table(routes: Vec<MockRoute>) -> MockRouteTable {
    MockRouteTable::new(routes)
}

fn spec_table() -> MockRouteTable {
    table(routes_from_openapi(SPEC).unwrap())
}

/// Sends a raw GET over loopback and returns the status code and body. Raw sockets keep the test free of a
/// client dependency, and make it obvious that only 127.0.0.1 is contacted.
fn get_over_loopback(port: u16, target: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(stream, "GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").unwrap();

    let mut raw = String::new();
    stream.read_to_string(&mut raw).unwrap();
    let status = raw.split_whitespace().nth(1).unwrap().parse().unwrap();
    let text = raw.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default();
    (status, text)
}

#[test]
fn literal_path_matches_only_the_same_method_and_segments() {
    let routes = table(vec![route("GET", "/pets", 200, "")]);

    assert!(routes.find("get", "/pets").is_some());
    assert!(routes.find("GET", "/pets/1").is_none());
    assert!(routes.find("GET", "/pet").is_none());
}

#[test]
fn query_string_and_trailing_slash_are_ignored() {
    let routes = table(vec![route("GET", "/pets", 200, "")]);

    assert!(routes.find("GET", "/pets?limit=2&key=secret").is_some());
    assert!(routes.find("GET", "/pets/").is_some());
    assert!(routes.find("GET", "http://127.0.0.1:8080/pets#top").is_some());
}

#[test]
fn parameter_segments_match_any_one_segment_in_all_three_styles() {
    for template in ["/pets/{id}", "/pets/{{id}}", "/pets/:id"] {
        let routes = table(vec![route("GET", template, 200, "")]);

        assert!(routes.find("GET", "/pets/42").is_some(), "{template}");
        assert!(routes.find("GET", "/pets/42/toys").is_none(), "{template}");
        assert!(routes.find("GET", "/pets").is_none(), "{template}");
    }
}

#[test]
fn a_literal_route_beats_a_parameter_route_on_the_same_path() {
    let routes = table(vec![
        route("GET", "/pets/{id}", 200, "by id"),
        route("GET", "/pets/search", 200, "search"),
    ]);

    assert_eq!(body(&routes.respond("GET", "/pets/search")), "search");
    assert_eq!(body(&routes.respond("GET", "/pets/7")), "by id");
}

#[test]
fn where_two_routes_tie_the_first_registered_wins() {
    let routes = table(vec![route("GET", "/a/{x}", 200, "first"), route("GET", "/a/{y}", 200, "second")]);

    assert_eq!(body(&routes.respond("GET", "/a/1")), "first");
}

#[test]
fn an_unknown_path_gets_a_404_as_json_without_echoing_the_query() {
    let reply = table(vec![route("GET", "/pets", 200, "")]).respond("GET", "/owners?key=abc");

    assert_eq!(reply.status_code, 404);
    assert_eq!(reply.media_type.as_deref(), Some("application/json; charset=utf-8"));
    let parsed: Value = serde_json::from_slice(&reply.body).unwrap();
    assert_eq!(parsed["error"], "No mock matches GET /owners.");
    assert_eq!(parsed["source"], "Ricochet mock server");
    assert!(!body(&reply).contains("abc"));
}

#[test]
fn a_known_path_with_another_method_gets_a_405_listing_the_allowed_methods() {
    let routes = table(vec![route("GET", "/pets/{id}", 200, ""), route("DELETE", "/pets/{id}", 204, "")]);

    let reply = routes.respond("PATCH", "/pets/3");

    assert_eq!(reply.status_code, 405);
    assert!(reply.headers.contains(&KeyValue::new("Allow", "DELETE, GET")));
}

#[test]
fn examples_become_routes_keyed_on_the_path_of_their_url() {
    let examples = [
        MockExample {
            method: "get".into(),
            url: "{{baseUrl}}/pets/{{petId}}?verbose=1".into(),
            status_code: 200,
            media_type: Some("application/json".into()),
            body: r#"{"id":1}"#.into(),
            headers: Vec::new(),
        },
        MockExample {
            method: "GET".into(),
            url: "https://api.example.test/pets/9".into(),
            status_code: 200,
            media_type: Some("application/json".into()),
            body: r#"{"id":9}"#.into(),
            headers: Vec::new(),
        },
        MockExample {
            method: "GET".into(),
            url: "/pets/{{petId}}".into(),
            status_code: 500,
            media_type: Some("text/plain".into()),
            body: "second, skipped".into(),
            headers: Vec::new(),
        },
    ];

    let routes = routes_from_examples(&examples);
    let table = MockRouteTable::new(routes.clone());

    assert_eq!(routes.len(), 2);
    // Only the first example for GET /pets/{id} is kept, so the 500 one is never sent.
    assert_eq!(body(&table.respond("GET", "/pets/5")), r#"{"id":1}"#);
    assert_eq!(table.respond("GET", "/pets/5").status_code, 200);
    assert_eq!(body(&table.respond("GET", "/pets/9")), r#"{"id":9}"#);
}

#[test]
fn a_description_produces_one_route_per_operation_under_the_server_path() {
    let routes = routes_from_openapi(SPEC).unwrap();
    let table = MockRouteTable::new(routes.clone());

    assert_eq!(routes.len(), 5);
    assert!(table.find("GET", "/v1/pets").is_some());
    assert!(table.find("POST", "/v1/pets").is_some());
    assert!(table.find("DELETE", "/v1/pets/3").is_some());
    assert!(table.find("GET", "/pets").is_none());
}

#[test]
fn a_response_uses_the_media_type_example_then_examples_then_a_schema_sample() {
    let table = spec_table();

    let listed: Value = serde_json::from_str(&body(&table.respond("GET", "/v1/pets"))).unwrap();
    assert_eq!(listed, json!([{ "id": 1, "name": "Rex" }]));

    let one: Value = serde_json::from_str(&body(&table.respond("GET", "/v1/pets/3"))).unwrap();
    assert_eq!(one, json!({ "id": 1, "name": "Rex" }));
}

#[test]
fn a_schema_sample_follows_references_allof_enums_and_formats() {
    let reply = spec_table().respond("POST", "/v1/pets");

    assert_eq!(reply.status_code, 201);
    assert_eq!(reply.media_type.as_deref(), Some("application/json"));
    let sample: Value = serde_json::from_slice(&reply.body).unwrap();
    assert_eq!(
        sample,
        json!({
            "name": "string",
            "age": 0,
            "active": true,
            "tags": ["string"],
            "owner": { "fullName": "string", "email": "user@example.com" },
            "status": "sold",
            "born": "2026-01-01"
        })
    );
}

#[test]
fn a_204_has_an_empty_body_and_no_media_type() {
    let reply = spec_table().respond("DELETE", "/v1/pets/3");

    assert_eq!(reply.status_code, 204);
    assert!(reply.body.is_empty());
    assert_eq!(reply.media_type, None);
}

#[test]
fn a_non_json_example_is_sent_as_text_and_the_default_response_answers_as_200() {
    let reply = spec_table().respond("GET", "/v1/pets/search");

    assert_eq!(reply.status_code, 200);
    assert_eq!(reply.media_type.as_deref(), Some("text/plain"));
    assert_eq!(body(&reply), "found");
}

#[test]
fn text_that_is_not_openapi_3_is_refused_with_a_message_for_the_user() {
    let not_json = routes_from_openapi("openapi: 3.0.0").unwrap_err();
    assert!(not_json.message().contains("not JSON"));

    let swagger2 = routes_from_openapi(r#"{"swagger":"2.0","paths":{}}"#).unwrap_err();
    assert_eq!(swagger2.message(), "This is not an OpenAPI 3 description.");

    let no_paths = routes_from_openapi(r#"{"openapi":"3.0.0"}"#).unwrap_err();
    assert!(no_paths.message().contains("no paths"));
}

#[test]
fn the_server_listens_on_loopback_only() {
    let mut server = MockServer::new();
    server.listen(0, MockRouteTable::default(), |_| {}).unwrap();

    let address = server.address().unwrap();
    assert!(address.ip().is_loopback());
    assert_eq!(address.ip().to_string(), "127.0.0.1");
    assert_eq!(server.port(), Some(address.port()));

    server.stop();
}

#[test]
fn the_server_answers_over_loopback_and_logs_the_path_without_the_query() {
    let mut server = MockServer::new();
    let (tx, rx) = mpsc::channel();
    server
        .listen(0, table(vec![route("GET", "/pets/{id}", 200, r#"{"ok":true}"#)]), move |entry| {
            tx.send(entry).unwrap();
        })
        .unwrap();
    let port = server.port().unwrap();

    let (hit_status, hit_body) = get_over_loopback(port, "/pets/4?key=secret");
    let (miss_status, _) = get_over_loopback(port, "/nope");

    assert_eq!(hit_status, 200);
    assert_eq!(hit_body, r#"{"ok":true}"#);
    assert_eq!(miss_status, 404);

    let first = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let second = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!((first.path.as_str(), first.status_code), ("/pets/4", 200));
    assert_eq!((second.path.as_str(), second.status_code), ("/nope", 404));
    assert_eq!(first.method, "GET");
    assert!(!first.path.contains("secret"));

    server.stop();
    assert!(!server.is_running());
}

#[test]
fn stop_frees_the_port_so_connections_are_refused_afterwards() {
    let mut server = MockServer::new();
    server.listen(0, MockRouteTable::default(), |_| {}).unwrap();
    let port = server.port().unwrap();

    server.stop();

    // Binding the port proves it is free. A refused connect would also show it, but Windows can take two
    // seconds to refuse one, so the test uses the faster check.
    assert!(TcpListener::bind(("127.0.0.1", port)).is_ok());
    assert_eq!(server.port(), None);
}

#[test]
fn update_routes_changes_the_answers_for_later_requests() {
    let mut server = MockServer::new();
    server.listen(0, table(vec![route("GET", "/a", 200, "before")]), |_| {}).unwrap();
    let port = server.port().unwrap();

    assert_eq!(get_over_loopback(port, "/a").1, "before");

    server.update_routes(table(vec![route("GET", "/a", 200, "after")]));

    assert_eq!(get_over_loopback(port, "/a").1, "after");
    server.stop();
}

#[test]
fn start_refuses_ports_outside_the_user_range() {
    let mut server = MockServer::new();

    let low = server.start(80, MockRouteTable::default(), |_| {}).unwrap_err();
    assert_eq!(low.message(), "Choose a port from 1024 to 65535.");

    let zero = server.start(0, MockRouteTable::default(), |_| {}).unwrap_err();
    assert_eq!(zero.message(), "Choose a port from 1024 to 65535.");
    assert!(!server.is_running());
}

#[test]
fn a_second_start_while_running_is_refused() {
    let mut server = MockServer::new();
    server.listen(0, MockRouteTable::default(), |_| {}).unwrap();

    let err = server.start(50000, MockRouteTable::default(), |_| {}).unwrap_err();
    assert_eq!(err.message(), "The mock server is already running.");

    server.stop();
}

#[test]
fn a_port_already_in_use_is_reported_for_the_user() {
    let mut first = MockServer::new();
    first.listen(0, MockRouteTable::default(), |_| {}).unwrap();
    let taken = first.port().unwrap();

    let mut second = MockServer::new();
    let err = second.listen(taken, MockRouteTable::default(), |_| {}).unwrap_err();
    assert_eq!(
        err.message(),
        format!("Port {taken} could not be used. Another program may already be using it.")
    );

    first.stop();
}
