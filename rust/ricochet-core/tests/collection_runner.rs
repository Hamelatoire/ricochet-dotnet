//! Ports the parts of `tests/Ricochet.Core.Tests/RunCombinationTests.cs`, `HiddenRunTests.cs` and `CollectionRunner`
//! behaviour that the Rust runner covers: run order, checks, captures, Variable layering, hidden Folders, tags,
//! masking and exit codes. The Requests go to a mock server on 127.0.0.1, which logs the paths it answers.

use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use pretty_assertions::assert_eq;
use ricochet_core::assertions::{RequestAssertion, RequestCapture, ops, sources};
use ricochet_core::collection_runner::{
    CollectionRunner, EXIT_CANNOT_RUN, EXIT_PASSED, EXIT_TESTS_FAILED, RunOptions, RunReport, RunSetupError, cautions, exit_code,
    summary,
};
use ricochet_core::mock_server::{MockReply, MockRoute, MockRouteTable, MockServer};
use ricochet_core::models::{ApiEnvironment, ApiRequest, Collection, CollectionItem, KeyValue, StoredScript, Workspace};
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

    /// The absolute URL for a path on this site.
    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    /// Stops the server, which waits for the last answer to be logged, and returns the paths it answered in order.
    fn finish(mut self) -> Vec<String> {
        self.server.stop();
        self.paths.lock().unwrap().clone()
    }
}

fn reply(status: u16, body: &str) -> MockReply {
    MockReply {
        status_code: status,
        headers: Vec::new(),
        body: body.as_bytes().to_vec(),
        media_type: Some("application/json".to_string()),
    }
}

fn route(method: &str, path: &str, status: u16, body: &str) -> MockRoute {
    MockRoute::new(method, path, reply(status, body))
}

fn ok(path: &str) -> MockRoute {
    route("GET", path, 200, "{}")
}

fn request(name: &str, url: &str) -> ApiRequest {
    ApiRequest { name: name.to_string(), method: "GET".to_string(), url: url.to_string(), ..ApiRequest::default() }
}

fn item(id: &str, url: &str) -> CollectionItem {
    CollectionItem { id: id.to_string(), name: id.to_string(), request: Some(request(id, url)), ..CollectionItem::default() }
}

fn folder(id: &str, tags: Option<&[&str]>, children: Vec<CollectionItem>) -> CollectionItem {
    CollectionItem {
        id: id.to_string(),
        name: id.to_string(),
        children,
        tags: tags.map(|t| t.iter().map(|s| s.to_string()).collect()),
        ..CollectionItem::default()
    }
}

fn collection(name: &str, items: Vec<CollectionItem>) -> Collection {
    Collection { name: name.to_string(), items, ..Collection::default() }
}

fn workspace(collections: Vec<Collection>) -> Workspace {
    Workspace { collections, ..Workspace::default() }
}

fn status_check(expected: &str) -> RequestAssertion {
    RequestAssertion { expected: expected.to_string(), ..RequestAssertion::default() }
}

fn json_capture(path: &str, variable: &str) -> RequestCapture {
    RequestCapture {
        source: sources::JSON.to_string(),
        path: path.to_string(),
        variable: variable.to_string(),
        ..RequestCapture::default()
    }
}

fn run(workspace: &Workspace, options: &RunOptions) -> Result<RunReport, RunSetupError> {
    let sender = RequestSender::new();
    CollectionRunner::new(&sender).run(workspace, options)
}

fn with_request_assertions(mut request: ApiRequest, assertions: Vec<RequestAssertion>) -> ApiRequest {
    request.assertions = Some(assertions);
    request
}

// ---- run order and selection ----

#[test]
fn requests_run_in_collection_and_folder_order_and_are_reported_by_path() {
    let site = Site::new(vec![ok("/a"), ok("/b"), ok("/c")]);
    let ws = workspace(vec![collection(
        "Shop",
        vec![
            item("A", &site.url("/a")),
            folder("F", None, vec![item("B", &site.url("/b"))]),
            item("C", &site.url("/c")),
        ],
    )]);

    let report = run(&ws, &RunOptions::default()).unwrap();

    assert!(report.passed());
    let paths: Vec<&str> = report.results.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(paths, ["A", "F / B", "C"]);
    assert_eq!(site.finish(), ["/a", "/b", "/c"]);
}

#[test]
fn collection_variables_override_globals_only_inside_their_own_collection() {
    let site = Site::new(vec![ok("/shop/x"), ok("/global/y")]);
    let ws = Workspace {
        globals: vec![KeyValue::new("prefix", "/global")],
        collections: vec![
            Collection {
                variables: vec![KeyValue::new("prefix", "/shop")],
                ..collection("Shop", vec![item("A", &format!("{}{{{{prefix}}}}/x", site.url("")))])
            },
            collection("Other", vec![item("B", &format!("{}{{{{prefix}}}}/y", site.url("")))]),
        ],
        ..Workspace::default()
    };

    let report = run(&ws, &RunOptions::default()).unwrap();

    assert!(report.passed());
    assert_eq!(site.finish(), ["/shop/x", "/global/y"]);
}

#[test]
fn an_environment_overrides_the_collection_and_run_overrides_win_over_everything() {
    let site = Site::new(vec![ok("/env/x"), ok("/given/x")]);
    let ws = Workspace {
        environments: vec![ApiEnvironment {
            name: "Staging".to_string(),
            variables: vec![KeyValue::new("seg", "/env")],
            ..ApiEnvironment::default()
        }],
        collections: vec![Collection {
            variables: vec![KeyValue::new("seg", "/collection")],
            ..collection("Shop", vec![item("A", &format!("{}{{{{seg}}}}/x", site.url("")))])
        }],
        ..Workspace::default()
    };

    // The environment name is matched without regard to case.
    run(&ws, &RunOptions { environment_name: Some("staging".to_string()), ..RunOptions::default() }).unwrap();
    run(
        &ws,
        &RunOptions {
            environment_name: Some("Staging".to_string()),
            overrides: vec![KeyValue::new("seg", "/given")],
            ..RunOptions::default()
        },
    )
    .unwrap();

    assert_eq!(site.finish(), ["/env/x", "/given/x"]);
}

#[test]
fn an_unknown_environment_is_refused_and_nothing_is_sent() {
    let site = Site::new(vec![ok("/a")]);
    let ws = Workspace {
        environments: vec![ApiEnvironment { name: "Staging".to_string(), ..ApiEnvironment::default() }],
        collections: vec![collection("Shop", vec![item("A", &site.url("/a"))])],
        ..Workspace::default()
    };

    let error = run(&ws, &RunOptions { environment_name: Some("Prod".to_string()), ..RunOptions::default() }).unwrap_err();

    assert_eq!(error.message(), "No environment is named \"Prod\". Available: Staging");
    assert!(site.finish().is_empty());
}

#[test]
fn a_run_with_nothing_to_run_is_refused() {
    let error = run(&Workspace::default(), &RunOptions::default()).unwrap_err();

    assert_eq!(error.message(), "There are no requests to run.");
}

#[test]
fn a_folder_name_limits_the_run_and_an_unknown_one_is_refused() {
    let site = Site::new(vec![ok("/b"), ok("/c")]);
    let ws = workspace(vec![collection(
        "Shop",
        vec![
            folder("Open", None, vec![item("B", &site.url("/b"))]),
            item("C", &site.url("/c")),
        ],
    )]);

    let report = run(&ws, &RunOptions { folder_name: Some("open".to_string()), ..RunOptions::default() }).unwrap();
    assert_eq!(report.results.len(), 1);
    assert_eq!(site.finish(), ["/b"]);

    let error = run(&ws, &RunOptions { folder_name: Some("nope".to_string()), ..RunOptions::default() }).unwrap_err();
    assert_eq!(error.message(), "No requests were found in a folder named \"nope\".");
}

#[test]
fn a_collection_name_limits_the_run_and_an_unknown_one_lists_the_available_names() {
    let site = Site::new(vec![ok("/a")]);
    let ws = workspace(vec![collection("Shop", vec![item("A", &site.url("/a"))]), collection("Other", vec![])]);

    let error = run(&ws, &RunOptions { collection_name: Some("Nope".to_string()), ..RunOptions::default() }).unwrap_err();
    assert_eq!(error.message(), "No collection is named \"Nope\". Available: Shop, Other");

    let report = run(&ws, &RunOptions { collection_name: Some("shop".to_string()), ..RunOptions::default() }).unwrap();
    assert_eq!(report.results.len(), 1);
    assert_eq!(site.finish(), ["/a"]);
}

#[test]
fn tags_select_requests_and_exclusion_wins_over_inclusion() {
    let site = Site::new(vec![ok("/a"), ok("/b")]);
    let ws = workspace(vec![collection(
        "Shop",
        vec![
            folder("Smoke", Some(&["smoke"]), vec![item("A", &site.url("/a"))]),
            folder("Slow", Some(&["slow"]), vec![item("B", &site.url("/b"))]),
        ],
    )]);

    let report = run(&ws, &RunOptions { include_tags: vec!["SMOKE".to_string()], ..RunOptions::default() }).unwrap();
    assert_eq!(report.results.len(), 1);

    let error = run(
        &ws,
        &RunOptions {
            include_tags: vec!["smoke".to_string()],
            exclude_tags: vec!["smoke".to_string()],
            ..RunOptions::default()
        },
    )
    .unwrap_err();
    assert_eq!(error.message(), "No requests match the tags given.");
    assert_eq!(site.finish(), ["/a"]);
}

// ---- hidden Folders (ADR 0032) ----

#[test]
fn a_hidden_folder_is_skipped_and_named_while_visible_requests_still_run() {
    let site = Site::new(vec![ok("/visible")]);
    let ws = workspace(vec![Collection {
        hidden_item_ids: Some(vec!["Hidden".to_string()]),
        ..collection(
            "Shop",
            vec![
                folder("Hidden", None, vec![item("Inside", &site.url("/inside"))]),
                item("Visible", &site.url("/visible")),
            ],
        )
    }]);

    let report = run(&ws, &RunOptions::default()).unwrap();

    assert!(report.passed());
    assert_eq!(report.skipped_hidden.len(), 1);
    assert_eq!(report.skipped_hidden[0].path, "Hidden / Inside");
    assert!(summary(&report).ends_with("1 request skipped because hidden."));
    assert!(cautions(&report).iter().any(|c| c.contains("Shop / Hidden / Inside")));
    assert_eq!(site.finish(), ["/visible"]);
}

#[test]
fn when_the_only_request_to_run_is_hidden_the_run_is_refused_and_nothing_is_sent() {
    let site = Site::new(vec![ok("/inside")]);
    let ws = workspace(vec![Collection {
        hidden_item_ids: Some(vec!["Hidden".to_string()]),
        ..collection("Shop", vec![folder("Hidden", None, vec![item("Inside", &site.url("/inside"))])])
    }]);

    let error = run(&ws, &RunOptions::default()).unwrap_err();

    assert_eq!(error.message(), "The only request that would run is hidden. Show it in the app to run it.");
    assert!(site.finish().is_empty());
}

#[test]
fn a_hidden_request_left_out_by_the_tag_filter_is_not_reported_as_skipped() {
    let site = Site::new(vec![ok("/visible")]);
    let ws = workspace(vec![Collection {
        hidden_item_ids: Some(vec!["Hidden".to_string()]),
        ..collection(
            "Shop",
            vec![
                folder("Hidden", Some(&["slow"]), vec![item("Inside", &site.url("/inside"))]),
                folder("Open", Some(&["smoke"]), vec![item("Visible", &site.url("/visible"))]),
            ],
        )
    }]);

    let report = run(&ws, &RunOptions { include_tags: vec!["smoke".to_string()], ..RunOptions::default() }).unwrap();

    assert!(report.skipped_hidden.is_empty());
    assert_eq!(site.finish(), ["/visible"]);
}

// ---- checks, captures and masking ----

#[test]
fn assertions_are_checked_in_order_and_a_failing_one_fails_the_request() {
    let site = Site::new(vec![route("GET", "/a", 200, "plain text")]);
    let a = with_request_assertions(
        request("A", &site.url("/a")),
        vec![
            status_check("200"),
            RequestAssertion { source: sources::BODY.to_string(), op: ops::CONTAINS.to_string(), expected: "nope".to_string(), ..RequestAssertion::default() },
        ],
    );
    let ws = workspace(vec![collection(
        "Shop",
        vec![CollectionItem { id: "A".to_string(), name: "A".to_string(), request: Some(a), ..CollectionItem::default() }],
    )]);

    let report = run(&ws, &RunOptions::default()).unwrap();

    let result = &report.results[0];
    assert_eq!(result.assertions.len(), 2);
    assert!(result.assertions[0].passed);
    assert!(!result.assertions[1].passed);
    assert!(!result.passed());
    assert!(!report.passed());
    assert_eq!(summary(&report), "1 request: 0 passed, 1 failed. 2 assertions: 1 passed, 1 failed.");
    site.finish();
}

#[test]
fn a_capture_feeds_a_later_request_in_the_same_collection_and_only_its_name_is_reported() {
    let site = Site::new(vec![route("POST", "/login", 200, r#"{"token":"abc123"}"#), ok("/me/abc123")]);
    let login = with_request_assertions(request("Login", &site.url("/login")), vec![]);
    let login = ApiRequest { method: "POST".to_string(), captures: Some(vec![json_capture("token", "token")]), ..login };
    let ws = workspace(vec![collection(
        "Shop",
        vec![
            CollectionItem { id: "Login".to_string(), name: "Login".to_string(), request: Some(login), ..CollectionItem::default() },
            item("Me", &format!("{}{{{{token}}}}", site.url("/me/"))),
        ],
    )]);

    let report = run(&ws, &RunOptions::default()).unwrap();

    assert!(report.passed());
    assert_eq!(report.results[0].captured, ["token"]);
    assert!(!summary(&report).contains("abc123"));
    assert_eq!(site.finish(), ["/login", "/me/abc123"]);
}

#[test]
fn captured_values_reset_for_each_collection() {
    let site = Site::new(vec![route("POST", "/login", 200, r#"{"token":"abc123"}"#)]);
    let login = ApiRequest {
        method: "POST".to_string(),
        captures: Some(vec![json_capture("token", "token")]),
        ..request("Login", &site.url("/login"))
    };
    let ws = workspace(vec![
        collection(
            "One",
            vec![CollectionItem { id: "Login".to_string(), name: "Login".to_string(), request: Some(login), ..CollectionItem::default() }],
        ),
        collection("Two", vec![item("Me", &format!("{}/me/{{{{token}}}}", site.url("")))]),
    ]);

    let report = run(&ws, &RunOptions::default()).unwrap();

    assert!(report.results[0].passed());
    assert_eq!(report.results[1].error.as_deref(), Some("Not sent, because there is no value for: token"));
    assert_eq!(site.finish(), ["/login"]);
}

#[test]
fn a_capture_that_finds_nothing_fails_its_request_and_captures_nothing() {
    let site = Site::new(vec![route("GET", "/a", 200, r#"{"other":"x"}"#)]);
    let a = ApiRequest {
        captures: Some(vec![json_capture("token", "token")]),
        ..request("A", &site.url("/a"))
    };
    let ws = workspace(vec![collection(
        "Shop",
        vec![CollectionItem { id: "A".to_string(), name: "A".to_string(), request: Some(a), ..CollectionItem::default() }],
    )]);

    let report = run(&ws, &RunOptions::default()).unwrap();

    let result = &report.results[0];
    assert!(!result.passed());
    assert!(result.captured.is_empty());
    assert_eq!(result.assertions.len(), 1);
    assert_eq!(result.assertions[0].message, "not found");
    site.finish();
}

#[test]
fn a_secret_is_masked_in_the_messages_of_failed_checks() {
    let site = Site::new(vec![route("GET", "/a", 200, "leaked collection-secret-value here")]);
    let a = with_request_assertions(
        request("A", &site.url("/a")),
        vec![RequestAssertion { source: sources::BODY.to_string(), op: ops::CONTAINS.to_string(), expected: "nope".to_string(), ..RequestAssertion::default() }],
    );
    let ws = workspace(vec![Collection {
        variables: vec![KeyValue { is_secret: true, ..KeyValue::new("token", "collection-secret-value") }],
        ..collection(
            "Shop",
            vec![CollectionItem { id: "A".to_string(), name: "A".to_string(), request: Some(a), ..CollectionItem::default() }],
        )
    }]);

    let report = run(&ws, &RunOptions::default()).unwrap();

    let check = &report.results[0].assertions[0];
    assert!(!check.passed);
    assert!(!check.message.contains("collection-secret-value"), "message leaked: {}", check.message);
    assert!(!check.description.contains("collection-secret-value"));
    assert!(check.message.contains("***"), "message was not masked: {}", check.message);
    site.finish();
}

#[test]
fn a_secret_captured_value_is_masked_in_the_same_responses_results() {
    let site = Site::new(vec![route("GET", "/a", 200, r#"{"token":"tok-987654"}"#)]);
    let a = with_request_assertions(
        request("A", &site.url("/a")),
        vec![RequestAssertion { source: sources::BODY.to_string(), op: ops::CONTAINS.to_string(), expected: "nope".to_string(), ..RequestAssertion::default() }],
    );
    let a = ApiRequest { captures: Some(vec![json_capture("token", "token")]), ..a };
    let ws = workspace(vec![collection(
        "Shop",
        vec![CollectionItem { id: "A".to_string(), name: "A".to_string(), request: Some(a), ..CollectionItem::default() }],
    )]);

    let report = run(&ws, &RunOptions::default()).unwrap();

    let check = &report.results[0].assertions[0];
    assert!(!check.passed);
    assert!(!check.message.contains("tok-987654"), "captured value leaked: {}", check.message);
    site.finish();
}

#[test]
fn an_unresolved_placeholder_stops_that_request_and_nothing_is_sent() {
    let site = Site::new(vec![ok("/a")]);
    let ws = workspace(vec![collection("Shop", vec![item("A", &format!("{}/{{{{missing}}}}", site.url("")))])]);

    let report = run(&ws, &RunOptions::default()).unwrap();

    assert_eq!(report.results[0].error.as_deref(), Some("Not sent, because there is no value for: missing"));
    assert!(!report.passed());
    assert!(site.finish().is_empty());
}

#[test]
fn a_run_stops_at_the_first_failure_when_asked_and_says_so() {
    let site = Site::new(vec![route("GET", "/a", 500, "{}"), ok("/b")]);
    let failing = with_request_assertions(request("A", &site.url("/a")), vec![status_check("200")]);
    let ws = workspace(vec![collection(
        "Shop",
        vec![
            CollectionItem { id: "A".to_string(), name: "A".to_string(), request: Some(failing), ..CollectionItem::default() },
            item("B", &site.url("/b")),
        ],
    )]);

    let stopped = run(&ws, &RunOptions { stop_on_first_failure: true, ..RunOptions::default() }).unwrap();
    assert_eq!(stopped.results.len(), 1);
    assert!(stopped.stopped);
    assert!(cautions(&stopped).iter().any(|c| c.starts_with("Stopped at the first failure")));

    let all = run(&ws, &RunOptions::default()).unwrap();
    assert_eq!(all.results.len(), 2);
    assert!(!all.stopped);
    assert_eq!(site.finish(), ["/a", "/a", "/b"]);
}

#[test]
fn the_callback_sees_each_result_as_it_finishes() {
    let site = Site::new(vec![ok("/a"), ok("/b")]);
    let ws = workspace(vec![collection("Shop", vec![item("A", &site.url("/a")), item("B", &site.url("/b"))])]);

    let sender = RequestSender::new();
    let mut seen: Vec<String> = Vec::new();
    let report = CollectionRunner::new(&sender)
        .run_with(&ws, &RunOptions::default(), |result| seen.push(result.path.clone()))
        .unwrap();

    assert_eq!(seen, ["A", "B"]);
    assert_eq!(report.results.len(), 2);
    site.finish();
}

#[test]
fn a_run_reads_the_workspace_and_never_changes_it() {
    let site = Site::new(vec![route("GET", "/a", 200, r#"{"token":"abc123"}"#)]);
    let a = ApiRequest { captures: Some(vec![json_capture("token", "token")]), ..request("A", &site.url("/a")) };
    let ws = Workspace {
        globals: vec![KeyValue::new("host", "x")],
        ..workspace(vec![collection(
            "Shop",
            vec![CollectionItem { id: "A".to_string(), name: "A".to_string(), request: Some(a), ..CollectionItem::default() }],
        )])
    };
    let before = ws.clone();

    run(&ws, &RunOptions::default()).unwrap();

    assert_eq!(ws, before);
    site.finish();
}

#[test]
fn requests_with_scripts_or_without_checks_are_cautioned_and_counted() {
    let site = Site::new(vec![ok("/a")]);
    let ws = workspace(vec![Collection {
        scripts: vec![StoredScript { listen: "test".to_string(), source: "pm.test()".to_string() }],
        ..collection("Shop", vec![item("A", &site.url("/a"))])
    }]);

    let report = run(&ws, &RunOptions::default()).unwrap();

    assert_eq!(report.requests_with_scripts(), 1);
    assert_eq!(report.requests_without_assertions(), 1);
    let found = cautions(&report);
    assert!(found.contains(&"1 request had Postman scripts that were not run, so any tests in them did not count.".to_string()));
    assert!(found.contains(&"1 request had no assertions, so only getting a response was checked.".to_string()));
    site.finish();
}

#[test]
fn a_request_that_cannot_connect_fails_alone_and_the_run_continues() {
    // A port that was free a moment ago and is closed now, so the connection is refused.
    let closed = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let site = Site::new(vec![ok("/b")]);
    let ws = workspace(vec![collection(
        "Shop",
        vec![item("A", &format!("http://127.0.0.1:{closed}/a")), item("B", &site.url("/b"))],
    )]);

    let report = run(&ws, &RunOptions::default()).unwrap();

    assert!(report.results[0].error.is_some());
    assert_eq!(report.results[0].status_code, None);
    assert!(!report.results[0].passed());
    assert!(report.results[1].passed());
    assert_eq!(site.finish(), ["/b"]);
}

// ---- exit codes and summary ----

#[test]
fn exit_codes_are_zero_for_a_pass_one_for_a_failed_request_and_two_for_a_run_that_could_not_happen() {
    let site = Site::new(vec![ok("/a"), route("GET", "/bad", 500, "{}")]);
    let passing = workspace(vec![collection("Shop", vec![item("A", &site.url("/a"))])]);
    let failing = workspace(vec![collection(
        "Shop",
        vec![CollectionItem {
            id: "Bad".to_string(),
            name: "Bad".to_string(),
            request: Some(with_request_assertions(request("Bad", &site.url("/bad")), vec![status_check("200")])),
            ..CollectionItem::default()
        }],
    )]);

    let pass = run(&passing, &RunOptions::default());
    let fail = run(&failing, &RunOptions::default());
    let cannot = run(&Workspace::default(), &RunOptions::default());

    assert_eq!(EXIT_PASSED, 0);
    assert_eq!(EXIT_TESTS_FAILED, 1);
    assert_eq!(EXIT_CANNOT_RUN, 2);
    assert_eq!(exit_code(&pass), EXIT_PASSED);
    assert_eq!(exit_code(&fail), EXIT_TESTS_FAILED);
    assert_eq!(exit_code(&cannot), EXIT_CANNOT_RUN);
    site.finish();
}

#[test]
fn the_summary_counts_requests_and_assertions_and_skipped_hidden_requests() {
    let site = Site::new(vec![ok("/a")]);
    let ws = workspace(vec![collection("Shop", vec![item("A", &site.url("/a"))])]);

    let report = run(&ws, &RunOptions::default()).unwrap();

    assert_eq!(summary(&report), "1 request: 1 passed, 0 failed. 0 assertions: 0 passed, 0 failed.");
    site.finish();
}
