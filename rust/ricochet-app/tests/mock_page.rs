use std::io::{Read, Write};
use std::net::TcpStream;

use pretty_assertions::assert_eq;
use ricochet_app::mock_page::{
    LOG_LIMIT, MockLog, active_table, examples_of, load_description, parse_port, read_description, source_text, start_table, table_of,
};
use ricochet_core::mock_server::{MockLogEntry, MockServer};
use ricochet_core::models::{ApiRequest, Collection, CollectionItem, ResponseExample, Workspace};

fn workspace() -> Workspace {
    let example = ResponseExample { name: "ok".into(), status_code: 201, body: "hello".into(), ..Default::default() };
    let request = ApiRequest {
        method: "GET".into(),
        url: "{{baseUrl}}/pets/{id}".into(),
        examples: Some(vec![example]),
        ..Default::default()
    };
    let inner = CollectionItem { name: "Pet".into(), request: Some(request), ..Default::default() };
    let folder = CollectionItem { name: "Folder".into(), children: vec![inner], ..Default::default() };
    let bare = CollectionItem { request: Some(ApiRequest { url: "/none".into(), ..Default::default() }), ..Default::default() };
    Workspace { collections: vec![Collection { items: vec![bare, folder], ..Default::default() }], ..Default::default() }
}

#[test]
fn examples_are_found_inside_folders_and_requests_without_examples_add_none() {
    let found = examples_of(&workspace());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].status_code, 201);
    assert_eq!(table_of(&workspace()).len(), 1);
}

#[test]
fn a_started_server_answers_with_the_example_and_logs_the_path_only() {
    let log = MockLog::default();
    let mut server = MockServer::new();
    let sink = log.clone();
    server.listen(0, table_of(&workspace()), move |entry| sink.push(entry)).unwrap();
    let port = server.port().unwrap();
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(stream, "GET /pets/7?token=secret HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    assert!(reply.starts_with("HTTP/1.1 201"), "{reply}");
    assert!(reply.ends_with("hello"));
    server.stop();
    let entries = log.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, "/pets/7");
}

#[test]
fn the_log_keeps_only_the_latest_entries_and_can_be_cleared() {
    let log = MockLog::default();
    for n in 0..LOG_LIMIT + 5 {
        log.push(MockLogEntry { at: chrono::Local::now(), method: "GET".into(), path: format!("/{n}"), status_code: 200 });
    }
    assert_eq!(log.entries().len(), LOG_LIMIT);
    assert_eq!(log.entries()[0].path, "/5");
    log.clear();
    assert!(log.entries().is_empty());
}

#[test]
fn ports_are_parsed_from_typed_text() {
    assert_eq!(parse_port(" 5050 "), Ok(5050));
    assert!(parse_port("abc").is_err());
    assert!(parse_port("70000").is_err());
}

const SPEC: &str = r#"{
  "openapi": "3.0.3",
  "servers": [ { "url": "https://api.example.test/v1" } ],
  "paths": {
    "/pets": { "get": { "responses": { "200": { "content": { "application/json": { "example": [ { "id": 1 } ] } } } } } },
    "/pets/{id}": { "get": { "responses": { "404": {}, "201": { "content": { "application/json": { "schema": { "type": "object", "properties": { "name": { "type": "string" } } } } } } } } },
    "/pets/mine": { "get": { "responses": { "200": { "content": { "text/plain": { "example": "mine" } } } } } }
  }
}"#;

#[test]
fn a_loaded_description_answers_instead_of_the_examples_with_the_core_precedence() {
    let description = load_description(SPEC, "pets.json").unwrap();
    let table = active_table(&workspace(), Some(&description));
    assert_eq!(table.len(), 3);
    // The first server's path is the base, and the lowest 2xx response answers with a sample from its schema.
    let reply = table.respond("GET", "/v1/pets/7");
    assert_eq!(reply.status_code, 201);
    assert_eq!(String::from_utf8(reply.body).unwrap(), r#"{"name":"string"}"#);
    // Literal segments win over parameters, and the query string is ignored.
    assert_eq!(table.respond("GET", "/v1/pets/mine?x=1").body, b"mine");
    assert_eq!(table.respond("POST", "/v1/pets").status_code, 405);
    // The examples' route is not served while the description is loaded.
    assert_eq!(table.respond("GET", "/pets/7").status_code, 404);
    assert_eq!(active_table(&workspace(), None).respond("GET", "/pets/7").status_code, 201);
}

#[test]
fn the_source_line_names_the_file_or_the_examples() {
    let description = load_description(SPEC, "pets.json").unwrap();
    assert_eq!(source_text(&workspace(), Some(&description)), "pets.json: 3 routes");
    assert_eq!(source_text(&workspace(), None), "Saved examples: 1 route");
}

#[test]
fn loading_errors_carry_the_core_messages() {
    assert_eq!(
        load_description("openapi: 3.0.0", "a.yaml").unwrap_err(),
        "This is not JSON. Ricochet reads OpenAPI 3 in JSON, and YAML is not supported yet."
    );
    assert_eq!(load_description(r#"{"swagger":"2.0","paths":{}}"#, "a.json").unwrap_err(), "This is not an OpenAPI 3 description.");
    assert_eq!(
        load_description(r#"{"openapi":"3.1.0"}"#, "a.json").unwrap_err(),
        "The description has no paths, so there is nothing to mock."
    );
}

#[test]
fn a_file_is_read_with_its_name_and_a_byte_order_mark_is_ignored() {
    let folder = std::env::temp_dir().join(format!("ricochet-mock-page-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("pets.json");
    std::fs::write(&path, format!("\u{feff}{SPEC}")).unwrap();
    let description = read_description(&path).unwrap();
    assert_eq!(description.file_name, "pets.json");
    assert_eq!(description.table.len(), 3);

    let missing = read_description(&folder.join("missing.json")).unwrap_err();
    assert!(missing.starts_with("The file could not be read: "), "{missing}");
    std::fs::remove_dir_all(&folder).ok();
}

#[test]
fn starting_needs_a_route_to_answer_with() {
    assert!(start_table(&Workspace::default(), None).unwrap_err().contains("no saved examples"));
    let no_operations = load_description(r#"{"openapi":"3.0.0","paths":{}}"#, "empty.json").unwrap();
    assert_eq!(
        start_table(&workspace(), Some(&no_operations)).unwrap_err(),
        "empty.json has no operations to answer with. Load another description."
    );
    assert_eq!(start_table(&workspace(), None).unwrap().len(), 1);
}
