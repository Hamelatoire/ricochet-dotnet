use std::io::{Read, Write};
use std::net::TcpStream;

use pretty_assertions::assert_eq;
use ricochet_app::mock_page::{LOG_LIMIT, MockLog, examples_of, parse_port, table_of};
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
