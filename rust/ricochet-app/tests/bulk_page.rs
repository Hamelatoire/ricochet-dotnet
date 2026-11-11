use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::Ordering;
use std::thread;

use pretty_assertions::assert_eq;
use ricochet_core::send::RequestSender;
use ricochet_app::bulk_page::{BulkShared, failed, finished_prefix, run};
use ricochet_core::bulk_runner::{BulkItemResult, parse_ids};
use ricochet_core::models::{ApiRequest, Collection, CollectionItem, Workspace};

/// A server that answers each of `count` connections with 200, or 500 for a path that ends in "bad".
fn server(count: usize) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for _ in 0..count {
            let Ok((mut stream, _)) = listener.accept() else { return };
            let mut buffer = [0u8; 2048];
            let read = stream.read(&mut buffer).unwrap_or(0);
            let head = String::from_utf8_lossy(&buffer[..read]).to_string();
            let status = if head.lines().next().unwrap_or("").contains("bad") { "500 Internal Server Error" } else { "200 OK" };
            let body = "ok";
            let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        }
    });
    port
}

fn workspace(port: u16) -> Workspace {
    let request = ApiRequest { id: "r".into(), name: "Get".into(), url: format!("http://127.0.0.1:{port}/items/{{{{id}}}}"), ..Default::default() };
    let item = CollectionItem { id: "item".into(), name: "Get".into(), request: Some(request), ..Default::default() };
    Workspace { collections: vec![Collection { id: "c".into(), name: "C".into(), items: vec![item], ..Default::default() }], ..Default::default() }
}

#[test]
fn one_request_is_sent_for_each_pasted_id_and_the_results_are_in_pasted_order() {
    let port = server(3);
    let ids = parse_ids("a\n\n  bad \nc\n");
    assert_eq!(ids, vec!["a", "bad", "c"]);
    let shared = BulkShared::default();
    let completed = run(&workspace(port), "item", None, &ids, "id", 1, &RequestSender::new(), &shared).unwrap();
    assert!(completed);
    assert!(shared.done.load(Ordering::SeqCst));

    let results = shared.results();
    assert_eq!(results.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["a", "bad", "c"]);
    assert_eq!(results.iter().map(|r| r.succeeded).collect::<Vec<_>>(), vec![true, false, true]);
    assert_eq!(failed(&results), 1);
    assert_eq!(finished_prefix(&results), 3);
}

#[test]
fn a_request_that_is_not_in_the_workspace_cannot_start_a_run_and_still_marks_it_done() {
    let shared = BulkShared::default();
    let outcome = run(&workspace(1), "missing", None, &["a".to_string()], "id", 1, &RequestSender::new(), &shared);
    assert!(outcome.is_err());
    assert!(shared.done.load(Ordering::SeqCst));
    assert!(shared.results().is_empty());
}

fn result(position: usize) -> BulkItemResult {
    BulkItemResult { id: position.to_string(), status_code: Some(200), succeeded: true, detail: String::new(), elapsed_ms: 1, position }
}

#[test]
fn a_gap_in_the_finished_ids_stops_the_resume_point_before_it() {
    assert_eq!(finished_prefix(&[]), 0);
    assert_eq!(finished_prefix(&[result(0), result(1), result(2)]), 3);
    assert_eq!(finished_prefix(&[result(2), result(0)]), 1, "position 1 never finished");
}
