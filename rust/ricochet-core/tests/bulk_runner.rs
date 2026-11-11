//! Ports `tests/Ricochet.Core.Tests/BulkRunnerTests.cs`. The ids are sent to a mock server on 127.0.0.1, which logs
//! the paths it answers, so the order of sends can be checked. Stopping and resume are checked with the shared flag.

use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use pretty_assertions::assert_eq;
use ricochet_core::bulk_runner::{BulkItemResult, BulkRun, BulkRunError, BulkRunner, DEFAULT_PLACEHOLDER, parse_ids, resume_index};
use ricochet_core::mock_server::{MockReply, MockRoute, MockRouteTable, MockServer};
use ricochet_core::models::{ApiRequest, Collection, KeyValue, Workspace};
use ricochet_core::send::RequestSender;

// ---- helpers ----

/// A mock server on 127.0.0.1 that records the path of every request it answers.
struct Site {
    server: MockServer,
    port: u16,
    paths: Arc<Mutex<Vec<String>>>,
}

impl Site {
    fn new(routes: Vec<MockRoute>) -> Self {
        let paths = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&paths);
        let mut server = MockServer::new();
        server
            .listen(0, MockRouteTable::new(routes), move |entry| log.lock().unwrap().push(entry.path))
            .expect("the mock server starts on loopback");
        let port = server.port().expect("a running server has a port");
        Self { server, port, paths }
    }

    /// Stops the server, which waits for the last answer to be logged, and returns the paths it answered in order.
    fn finish(mut self) -> Vec<String> {
        self.server.stop();
        self.paths.lock().unwrap().clone()
    }
}

fn reply(status: u16, body: &str) -> MockReply {
    MockReply { status_code: status, headers: Vec::new(), body: body.as_bytes().to_vec(), media_type: Some("text/plain".to_string()) }
}

/// A template whose URL holds the id: `{host}/products/{{id}}/sync`.
fn template(base: &str) -> ApiRequest {
    ApiRequest {
        name: "Sync".to_string(),
        method: "GET".to_string(),
        url: format!("{base}/products/{{{{id}}}}/sync"),
        ..ApiRequest::default()
    }
}

fn product_routes(ids: &[&str]) -> Vec<MockRoute> {
    ids.iter()
        .map(|id| MockRoute::new("GET", &format!("/products/{id}/sync"), reply(200, "")))
        .collect()
}

fn ids(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

/// Runs the template for each id, with the given thread count and overrides, and returns the results in the order they arrived.
fn run_bulk(
    template: &ApiRequest,
    collection: &Collection,
    list: &[String],
    max_parallel: usize,
    overrides: &[KeyValue],
    stop: &AtomicBool,
    mut on_result: impl FnMut(&BulkItemResult) + Send,
) -> Result<bool, BulkRunError> {
    let sender = RequestSender::new();
    let workspace = Workspace::default();
    let run = BulkRun {
        workspace: &workspace,
        collection,
        item_id: None,
        environment: None,
        template,
        placeholder: DEFAULT_PLACEHOLDER,
        ids: list,
        overrides,
        max_parallel,
    };
    BulkRunner::new(&sender).run(&run, stop, |result| on_result(&result))
}

fn collected(
    template: &ApiRequest,
    collection: &Collection,
    list: &[String],
    max_parallel: usize,
) -> (Vec<BulkItemResult>, bool) {
    let stop = AtomicBool::new(false);
    let mut results = Vec::new();
    let finished = run_bulk(template, collection, list, max_parallel, &[], &stop, |r| results.push(r.clone())).unwrap();
    (results, finished)
}

// ---- parsing and resume ----

#[test]
fn ids_are_split_per_line_and_blank_lines_are_dropped() {
    assert_eq!(parse_ids("  a1 \r\n\r\nb2\n   \nc3"), ["a1", "b2", "c3"]);
}

#[test]
fn a_run_resumes_after_its_last_id_and_starts_over_when_that_id_is_gone() {
    let list = ids(&["p1", "p2", "p3"]);

    assert_eq!(resume_index(&list, None), 0);
    assert_eq!(resume_index(&list, Some("p1")), 1);
    assert_eq!(resume_index(&list, Some("p3")), 3);
    assert_eq!(resume_index(&list, Some("gone")), 0);
}

// ---- sending ----

#[test]
fn one_request_is_sent_per_id_in_order_and_each_result_is_logged() {
    let site = Site::new({
        let mut routes = product_routes(&["p1", "p3"]);
        routes.push(MockRoute::new("GET", "/products/bad/sync", reply(404, "no such product")));
        routes
    });
    let collection = Collection { name: "Shop".to_string(), ..Collection::default() };

    let (results, finished) = collected(&template(&format!("http://127.0.0.1:{}", site.port)), &collection, &ids(&["p1", "bad", "p3"]), 1);

    assert!(finished);
    assert_eq!(results.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["p1", "bad", "p3"]);
    assert_eq!(results[0].log_line(), "OK  p1  [200]");
    assert_eq!(results[1].log_line(), "ERR bad  [404] no such product");
    assert_eq!(site.finish(), ["/products/p1/sync", "/products/bad/sync", "/products/p3/sync"]);
}

#[test]
fn stopping_after_an_id_sends_no_more() {
    let site = Site::new(product_routes(&["p1", "p2", "p3"]));
    let collection = Collection::default();
    let stop = AtomicBool::new(false);
    let mut results = Vec::new();

    let finished = run_bulk(
        &template(&format!("http://127.0.0.1:{}", site.port)),
        &collection,
        &ids(&["p1", "p2", "p3"]),
        1,
        &[],
        &stop,
        |result| {
            results.push(result.id.clone());
            stop.store(true, Ordering::SeqCst);
        },
    )
    .unwrap();

    assert!(!finished);
    assert_eq!(results, ["p1"]);
    assert_eq!(site.finish(), ["/products/p1/sync"]);
}

#[test]
fn a_stop_pressed_during_an_id_lets_that_id_finish_and_no_more_start() {
    let site = Site::new(product_routes(&["p1", "p2"]));
    let collection = Collection::default();
    let stop = AtomicBool::new(false);
    let mut results: Vec<BulkItemResult> = Vec::new();

    let finished = run_bulk(
        &template(&format!("http://127.0.0.1:{}", site.port)),
        &collection,
        &ids(&["p1", "p2"]),
        1,
        &[],
        &stop,
        |result| {
            results.push(result.clone());
            stop.store(true, Ordering::SeqCst);
        },
    )
    .unwrap();

    assert!(!finished);
    assert_eq!(results.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["p1"]);
    assert!(results[0].succeeded);
    assert_eq!(site.finish(), ["/products/p1/sync"]);
}

#[test]
fn a_parallel_run_reports_every_id_once_with_its_pasted_position() {
    let names: Vec<String> = (1..=8).map(|i| format!("p{i}")).collect();
    let routes = names.iter().map(|id| MockRoute::new("GET", &format!("/products/{id}/sync"), reply(200, ""))).collect();
    let site = Site::new(routes);
    let collection = Collection::default();

    let (results, finished) = collected(&template(&format!("http://127.0.0.1:{}", site.port)), &collection, &names, 3);

    assert!(finished);
    let mut seen: Vec<&str> = results.iter().map(|r| r.id.as_str()).collect();
    seen.sort_unstable();
    let mut expected: Vec<&str> = names.iter().map(String::as_str).collect();
    expected.sort_unstable();
    assert_eq!(seen, expected);
    let mut positions: Vec<usize> = results.iter().map(|r| r.position).collect();
    positions.sort_unstable();
    assert_eq!(positions, (0..names.len()).collect::<Vec<_>>());
    assert!(results.iter().all(|r| r.succeeded));
    site.finish();
}

#[test]
fn one_thread_sends_the_ids_in_the_pasted_order() {
    let site = Site::new(product_routes(&["p1", "p2", "p3"]));
    let collection = Collection::default();

    let (_, finished) = collected(&template(&format!("http://127.0.0.1:{}", site.port)), &collection, &ids(&["p1", "p2", "p3"]), 1);

    assert!(finished);
    assert_eq!(site.finish(), ["/products/p1/sync", "/products/p2/sync", "/products/p3/sync"]);
}

#[test]
fn a_run_that_asks_for_no_threads_is_refused() {
    let collection = Collection::default();
    let stop = AtomicBool::new(false);

    let error = run_bulk(&template("http://127.0.0.1:9"), &collection, &ids(&["p1"]), 0, &[], &stop, |_| {}).unwrap_err();

    assert_eq!(error, BulkRunError::NoThreads);
    assert_eq!(error.to_string(), "At least one thread is needed.");
}

#[test]
fn a_run_whose_request_never_uses_the_placeholder_sends_nothing() {
    // The URL uses {{productId}}, which a Collection Variable sets to empty, while the ids replace {{id}}. Every id
    // would send the same /products//sync, so the run is refused before anything is sent.
    let site = Site::new(Vec::new());
    let collection = Collection { variables: vec![KeyValue::new("productId", "")], ..Collection::default() };
    let bulk_template = ApiRequest {
        method: "GET".to_string(),
        url: format!("http://127.0.0.1:{}/products/{{{{productId}}}}/sync", site.port),
        ..ApiRequest::default()
    };
    let stop = AtomicBool::new(false);

    let error = run_bulk(&bulk_template, &collection, &ids(&["p1", "p2"]), 1, &[], &stop, |_| {}).unwrap_err();

    assert_eq!(error, BulkRunError::PlaceholderUnused("id".to_string()));
    assert!(error.to_string().contains("doesn't use {{id}}"), "{error}");
    assert_eq!(site.finish(), Vec::<String>::new());
}

#[test]
fn a_placeholder_reached_through_another_variable_counts_as_used() {
    let site = Site::new(product_routes(&["p1"]));
    let collection = Collection { variables: vec![KeyValue::new("productId", "{{id}}")], ..Collection::default() };
    let bulk_template = ApiRequest {
        method: "GET".to_string(),
        url: format!("http://127.0.0.1:{}/products/{{{{productId}}}}/sync", site.port),
        ..ApiRequest::default()
    };

    let (results, finished) = collected(&bulk_template, &collection, &ids(&["p1"]), 1);

    assert!(finished);
    assert!(results[0].succeeded, "{:?}", results[0]);
    assert_eq!(site.finish(), ["/products/p1/sync"]);
}

#[test]
fn each_id_is_sent_with_the_bearer_token_from_the_active_environment() {
    use std::io::{Read, Write};
    use ricochet_core::models::{ApiEnvironment, RequestAuth, auth_kinds};

    // A raw listener, because the mock server never logs headers. It answers 200 to each request and keeps the
    // Authorization line it received.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let received = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&received);
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = Vec::new();
            let mut chunk = [0u8; 1024];
            while !buffer.windows(4).any(|w| w == b"\r\n\r\n") {
                let read = stream.read(&mut chunk).unwrap();
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..read]);
            }
            let text = String::from_utf8_lossy(&buffer).to_string();
            let first = text.lines().next().unwrap_or_default().to_string();
            let auth = text.lines().find(|l| l.to_ascii_lowercase().starts_with("authorization:")).unwrap_or_default().to_string();
            log.lock().unwrap().push((first, auth));
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        }
    });

    let collection = Collection::default();
    let environment = ApiEnvironment { variables: vec![KeyValue::new("token", "env-token-123")], ..ApiEnvironment::default() };
    let bulk_template = ApiRequest {
        method: "GET".to_string(),
        url: format!("http://127.0.0.1:{port}/products/{{{{id}}}}/sync"),
        auth: Some(RequestAuth { kind: auth_kinds::BEARER.to_string(), token: "{{token}}".to_string(), ..RequestAuth::default() }),
        ..ApiRequest::default()
    };
    let list = ids(&["p1", "p2"]);
    let workspace = Workspace::default();
    let sender = RequestSender::new();
    let run = BulkRun {
        workspace: &workspace,
        collection: &collection,
        item_id: None,
        environment: Some(&environment),
        template: &bulk_template,
        placeholder: DEFAULT_PLACEHOLDER,
        ids: &list,
        overrides: &[],
        max_parallel: 1,
    };
    let stop = AtomicBool::new(false);
    let mut results = Vec::new();

    let finished = BulkRunner::new(&sender).run(&run, &stop, |result| results.push(result)).unwrap();
    server.join().unwrap();

    assert!(finished);
    assert!(results.iter().all(|r| r.succeeded), "{results:?}");
    assert_eq!(
        *received.lock().unwrap(),
        [
            ("GET /products/p1/sync HTTP/1.1".to_string(), "authorization: Bearer env-token-123".to_string()),
            ("GET /products/p2/sync HTTP/1.1".to_string(), "authorization: Bearer env-token-123".to_string()),
        ]
    );
}

#[test]
fn an_id_with_a_missing_variable_is_not_sent() {
    let site = Site::new(Vec::new());
    let collection = Collection::default();
    let bulk_template = ApiRequest {
        url: format!("http://127.0.0.1:{}/{{{{id}}}}/{{{{region}}}}", site.port),
        ..ApiRequest::default()
    };

    let (results, finished) = collected(&bulk_template, &collection, &ids(&["p1"]), 1);

    assert!(finished);
    assert!(!results[0].succeeded);
    assert_eq!(results[0].detail, "Not sent, because there is no value for: region");
    assert!(site.finish().is_empty());
}

#[test]
fn an_id_whose_connection_fails_fails_alone_and_the_others_still_run() {
    let closed = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let site = Site::new(product_routes(&["p1"]));
    let collection = Collection::default();
    // The id is the host, so one id points at a closed port and the others at the mock server.
    let bulk_template = ApiRequest {
        url: "http://{{id}}/products/p1/sync".to_string(),
        ..ApiRequest::default()
    };

    let good = format!("127.0.0.1:{}", site.port);
    let bad = format!("127.0.0.1:{closed}");
    let (results, finished) = collected(&bulk_template, &collection, &[good.clone(), bad, good], 1);

    assert!(finished);
    assert!(results[0].succeeded);
    assert!(!results[1].succeeded);
    assert_eq!(results[1].status_code, None);
    assert!(!results[1].detail.is_empty());
    assert!(results[2].succeeded);
    site.finish();
}

#[test]
fn a_failed_id_detail_is_the_start_of_the_body_on_one_line_and_is_cut_at_300_characters() {
    let long_body = format!("first line\n{}", "x".repeat(400));
    let site = Site::new(vec![MockRoute::new("GET", "/products/p1/sync", reply(500, &long_body))]);
    let collection = Collection::default();

    let (results, _) = collected(&template(&format!("http://127.0.0.1:{}", site.port)), &collection, &ids(&["p1"]), 1);

    let detail = &results[0].detail;
    assert!(detail.starts_with("first line x"), "detail was {detail}");
    assert_eq!(detail.chars().count(), 301);
    assert!(detail.ends_with('…'));
    site.finish();
}

#[test]
fn a_secret_variable_is_masked_in_the_detail_of_a_failed_id() {
    let site = Site::new(vec![MockRoute::new("GET", "/products/p1/sync", reply(401, "token sek-ret-123 was rejected"))]);
    let collection = Collection {
        name: "Shop".to_string(),
        variables: vec![KeyValue { is_secret: true, ..KeyValue::new("token", "sek-ret-123") }],
        ..Collection::default()
    };

    let (results, _) = collected(&template(&format!("http://127.0.0.1:{}", site.port)), &collection, &ids(&["p1"]), 1);

    assert!(!results[0].succeeded);
    assert!(!results[0].detail.contains("sek-ret-123"), "detail leaked: {}", results[0].detail);
    assert!(results[0].detail.contains("***"));
    site.finish();
}

#[test]
fn a_run_override_is_masked_in_the_detail_of_a_failed_id_like_a_secret() {
    let site = Site::new(vec![MockRoute::new("GET", "/products/p1/sync", reply(401, "key ovr-val-456 was rejected"))]);
    let collection = Collection::default();
    let stop = AtomicBool::new(false);
    let mut results = Vec::new();

    run_bulk(
        &template(&format!("http://127.0.0.1:{}", site.port)),
        &collection,
        &ids(&["p1"]),
        1,
        &[KeyValue::new("key", "ovr-val-456")],
        &stop,
        |r| results.push(r.clone()),
    )
    .unwrap();

    assert!(!results[0].detail.contains("ovr-val-456"), "detail leaked: {}", results[0].detail);
    assert!(results[0].detail.contains("***"));
    site.finish();
}
