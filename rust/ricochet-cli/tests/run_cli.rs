//! Runs the `ricochet` binary against a Workspace written to a temporary folder, and checks its output, exit codes and
//! xUnit report. Requests go only to a mock server on 127.0.0.1, or to a port that was just closed on 127.0.0.1.

use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use pretty_assertions::assert_eq;
use ricochet_core::assertions::{ops, sources, RequestAssertion};
use ricochet_core::mock_server::{MockReply, MockRoute, MockRouteTable, MockServer};
use ricochet_core::models::{ApiRequest, Collection, CollectionItem, Workspace};
use ricochet_core::store::WorkspaceFolder;
use uuid::Uuid;

// ---- helpers ----

/// A temporary folder that is removed when the test ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("ricochet-cli-{}", Uuid::new_v4().simple()));
        fs::create_dir_all(&path).expect("the temp folder can be created");
        Self(path)
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.file(name);
        fs::write(&path, text).expect("the test file can be written");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A mock server on 127.0.0.1. It stops when dropped.
struct Site {
    server: MockServer,
    port: u16,
}

impl Site {
    fn new(routes: Vec<MockRoute>) -> Self {
        let mut server = MockServer::new();
        server
            .listen(0, MockRouteTable::new(routes), |_entry| {})
            .expect("the mock server starts on loopback");
        let port = server.port().expect("a running server has a port");
        Self { server, port }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        self.server.stop();
    }
}

fn route(method: &str, path: &str, status: u16, body: &str) -> MockRoute {
    MockRoute::new(
        method,
        path,
        MockReply {
            status_code: status,
            headers: Vec::new(),
            body: body.as_bytes().to_vec(),
            media_type: Some("application/json".to_string()),
        },
    )
}

/// A status check: the response's status must equal `expected`.
fn status_is(expected: &str) -> RequestAssertion {
    RequestAssertion {
        source: sources::STATUS.to_string(),
        op: ops::EQUAL.to_string(),
        expected: expected.to_string(),
        ..RequestAssertion::default()
    }
}

fn request(name: &str, url: &str, assertions: Vec<RequestAssertion>) -> CollectionItem {
    CollectionItem {
        id: name.to_string(),
        name: name.to_string(),
        request: Some(ApiRequest {
            name: name.to_string(),
            method: "GET".to_string(),
            url: url.to_string(),
            assertions: Some(assertions),
            ..ApiRequest::default()
        }),
        ..CollectionItem::default()
    }
}

/// Writes a Workspace with one Collection named `Shop` into `folder`, so the binary can read it.
fn write_workspace(folder: &Path, items: Vec<CollectionItem>) {
    let workspace = Workspace {
        collections: vec![Collection { id: "shop".to_string(), name: "Shop".to_string(), items, ..Collection::default() }],
        ..Workspace::default()
    };
    WorkspaceFolder::new(folder).save(&workspace).expect("the workspace can be saved to a temp folder");
}

fn ricochet(args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ricochet"));
    command.args(args);
    command
}

fn run_ok(mut command: Command) -> Output {
    command.output().expect("the ricochet binary runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn exit_code(output: &Output) -> i32 {
    output.status.code().expect("the process exits with a code")
}

/// A port with nothing listening on it, on 127.0.0.1. Connecting to it is refused.
fn closed_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback is available");
    let port = listener.local_addr().expect("a bound listener has an address").port();
    drop(listener);
    port
}

// ---- help and argument errors ----

#[test]
fn run_help_prints_usage_and_exits_zero() {
    let output = run_ok(ricochet(&["run", "--help"]));

    assert_eq!(exit_code(&output), 0);
    let text = stdout(&output);
    assert!(text.contains("Usage:"), "help shows usage: {text}");
    assert!(text.contains("--xunit <file>"), "help lists --xunit: {text}");
    assert!(text.contains("--data <file>"), "help lists --data: {text}");
}

#[test]
fn no_arguments_prints_usage_and_exits_two() {
    let output = run_ok(ricochet(&[]));

    assert_eq!(exit_code(&output), 2);
    assert!(stdout(&output).contains("Usage:"));
}

#[test]
fn export_outside_a_workspace_exits_two() {
    let scratch = Scratch::new();
    let output = run_ok(ricochet(&["export", "Shop", "--workspace", scratch.0.to_str().unwrap(), "--postman", "x.json"]));

    assert_eq!(exit_code(&output), 2);
    assert!(stderr(&output).contains("is not a Ricochet workspace folder."), "stderr: {}", stderr(&output));
}

#[test]
fn unknown_option_exits_two() {
    let scratch = Scratch::new();
    let output = run_ok(ricochet(&["run", scratch.0.to_str().unwrap(), "--nope"]));

    assert_eq!(exit_code(&output), 2);
    assert!(stderr(&output).contains("Unknown option \"--nope\""));
}

#[test]
fn a_second_folder_argument_exits_two() {
    let output = run_ok(ricochet(&["run", "one", "two"]));

    assert_eq!(exit_code(&output), 2);
    assert!(stderr(&output).contains("Unexpected argument \"two\""));
}

#[test]
fn timeout_must_be_a_positive_number() {
    let scratch = Scratch::new();
    let output = run_ok(ricochet(&["run", scratch.0.to_str().unwrap(), "--timeout", "0"]));

    assert_eq!(exit_code(&output), 2);
    assert!(stderr(&output).contains("--timeout needs a positive number of seconds."));
}

#[test]
fn var_needs_name_and_value() {
    let scratch = Scratch::new();
    let output = run_ok(ricochet(&["run", scratch.0.to_str().unwrap(), "--var", "=oops"]));

    assert_eq!(exit_code(&output), 2);
    assert!(stderr(&output).contains("--var needs name=value."));
}

#[test]
fn cookies_are_accepted() {
    let scratch = Scratch::new();
    let output = run_ok(ricochet(&["run", scratch.0.to_str().unwrap(), "--cookies"]));

    // The option is understood, so the run gets as far as checking the folder.
    assert_eq!(exit_code(&output), 2);
    assert!(stderr(&output).contains("is not a Ricochet workspace folder."), "stderr: {}", stderr(&output));
}

#[test]
fn a_folder_that_is_not_a_workspace_exits_two() {
    let scratch = Scratch::new();
    let output = run_ok(ricochet(&["run", scratch.0.to_str().unwrap()]));

    assert_eq!(exit_code(&output), 2);
    assert!(stderr(&output).contains("is not a Ricochet workspace folder."));
}

// ---- runs ----

#[test]
fn a_passing_run_exits_zero_and_prints_the_summary() {
    let site = Site::new(vec![route("GET", "/ok", 200, "{}")]);
    let scratch = Scratch::new();
    write_workspace(&scratch.0, vec![request("A", &site.url("/ok"), vec![status_is("200")])]);

    let output = run_ok(ricochet(&["run", scratch.0.to_str().unwrap()]));

    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("Shop"), "prints the collection name: {text}");
    assert!(text.contains("PASS"), "prints the verdict: {text}");
    assert!(text.contains("200  "), "prints the status code: {text}");
    assert!(
        text.contains("1 request: 1 passed, 0 failed. 1 assertion: 1 passed, 0 failed."),
        "prints the summary: {text}"
    );
}

#[test]
fn a_failing_check_exits_one_and_prints_the_message() {
    let site = Site::new(vec![route("GET", "/ok", 200, "{}")]);
    let scratch = Scratch::new();
    write_workspace(&scratch.0, vec![request("A", &site.url("/ok"), vec![status_is("500")])]);

    let output = run_ok(ricochet(&["run", scratch.0.to_str().unwrap()]));

    assert_eq!(exit_code(&output), 1);
    let text = stdout(&output);
    assert!(text.contains("FAIL"), "prints the failing verdict: {text}");
    assert!(text.contains("1 assertion: 0 passed, 1 failed."), "prints the summary: {text}");
}

#[test]
fn a_request_that_cannot_connect_exits_one() {
    let scratch = Scratch::new();
    let url = format!("http://127.0.0.1:{}/", closed_port());
    write_workspace(&scratch.0, vec![request("A", &url, Vec::new())]);

    let output = run_ok(ricochet(&["run", scratch.0.to_str().unwrap()]));

    assert_eq!(exit_code(&output), 1);
    assert!(stdout(&output).contains("FAIL"));
}

#[test]
fn xunit_report_is_written_for_a_passing_run() {
    let site = Site::new(vec![route("GET", "/ok", 200, "{}")]);
    let scratch = Scratch::new();
    write_workspace(&scratch.0, vec![request("A", &site.url("/ok"), vec![status_is("200")])]);
    let report = scratch.file("results.xml");

    let output = run_ok(ricochet(&["run", scratch.0.to_str().unwrap(), "--xunit", report.to_str().unwrap()]));

    assert_eq!(exit_code(&output), 0);
    let xml = fs::read_to_string(&report).expect("the xUnit report was written");
    assert!(xml.starts_with("<?xml"), "xml: {xml}");
    assert!(xml.contains("<assemblies"), "xml: {xml}");
    assert!(xml.contains("<assembly name=\"Shop\""), "xml: {xml}");
    assert!(xml.contains("total=\"1\" passed=\"1\" failed=\"0\""), "xml: {xml}");
    assert!(xml.contains("result=\"Pass\""), "xml: {xml}");
    assert!(!xml.contains("<failure"), "a passing test has no failure: {xml}");
}

#[test]
fn xunit_report_lists_the_failure_message_for_a_failing_run() {
    let site = Site::new(vec![route("GET", "/ok", 200, "{}")]);
    let scratch = Scratch::new();
    write_workspace(&scratch.0, vec![request("A", &site.url("/ok"), vec![status_is("500")])]);
    let report = scratch.file("results.xml");

    let output = run_ok(ricochet(&["run", scratch.0.to_str().unwrap(), "--xunit", report.to_str().unwrap()]));

    assert_eq!(exit_code(&output), 1);
    let xml = fs::read_to_string(&report).expect("the xUnit report was written");
    assert!(xml.contains("result=\"Fail\""), "xml: {xml}");
    assert!(xml.contains("exception-type=\"AssertionFailed\""), "xml: {xml}");
    assert!(xml.contains("<![CDATA["), "xml: {xml}");
}

#[test]
fn an_unknown_environment_exits_two() {
    let scratch = Scratch::new();
    write_workspace(&scratch.0, vec![request("A", "http://127.0.0.1:1/", Vec::new())]);

    let output = run_ok(ricochet(&["run", scratch.0.to_str().unwrap(), "--env", "Nope"]));

    assert_eq!(exit_code(&output), 2);
    assert!(stderr(&output).contains("No environment is named \"Nope\""));
}

#[test]
fn a_var_option_sets_a_variable_for_the_run() {
    let site = Site::new(vec![route("GET", "/alice", 200, "{}")]);
    let scratch = Scratch::new();
    write_workspace(&scratch.0, vec![request("A", &site.url("/{{who}}"), vec![status_is("200")])]);

    let output = run_ok(ricochet(&["run", scratch.0.to_str().unwrap(), "--var", "who=alice"]));

    assert_eq!(exit_code(&output), 0, "stdout: {} stderr: {}", stdout(&output), stderr(&output));
}

#[test]
fn an_environment_variable_with_the_ricochet_prefix_sets_a_variable() {
    let site = Site::new(vec![route("GET", "/alice", 200, "{}")]);
    let scratch = Scratch::new();
    write_workspace(&scratch.0, vec![request("A", &site.url("/{{who}}"), vec![status_is("200")])]);

    let mut command = ricochet(&["run", scratch.0.to_str().unwrap()]);
    command.env("RICOCHET_VAR_who", "alice");
    let output = run_ok(command);

    assert_eq!(exit_code(&output), 0, "stdout: {} stderr: {}", stdout(&output), stderr(&output));
}

#[test]
fn a_secret_value_is_masked_in_the_output() {
    // The URL carries the value, and the connection is refused, so the failure text is the only output that could show it.
    let scratch = Scratch::new();
    let url = format!("http://127.0.0.1:{}/?k={{{{token}}}}", closed_port());
    write_workspace(&scratch.0, vec![request("A", &url, Vec::new())]);

    let output = run_ok(ricochet(&["run", scratch.0.to_str().unwrap(), "--var", "token=s3cr3t-value-123"]));

    assert_eq!(exit_code(&output), 1);
    let everything = format!("{}{}", stdout(&output), stderr(&output));
    assert!(!everything.contains("s3cr3t-value-123"), "the secret must not be printed: {everything}");
}

// ---- data files ----

#[test]
fn a_data_file_runs_once_per_row_and_joins_the_reports() {
    let site = Site::new(vec![route("GET", "/alice", 200, "{}"), route("GET", "/bob", 500, "{}")]);
    let scratch = Scratch::new();
    write_workspace(&scratch.0, vec![request("A", &site.url("/{{who}}"), vec![status_is("200")])]);
    let data = scratch.write("users.csv", "who\nalice\nbob\n");
    let report = scratch.file("results.xml");

    let output = run_ok(ricochet(&[
        "run",
        scratch.0.to_str().unwrap(),
        "--data",
        data.to_str().unwrap(),
        "--xunit",
        report.to_str().unwrap(),
    ]));

    assert_eq!(exit_code(&output), 1);
    let text = stdout(&output);
    assert!(text.contains("Row 1 of 2"), "stdout: {text}");
    assert!(text.contains("Row 2 of 2"), "stdout: {text}");
    assert!(text.contains("Data rows: 2 run, 1 passed, 1 failed."), "stdout: {text}");

    let xml = fs::read_to_string(&report).expect("the xUnit report was written");
    assert!(xml.contains("Shop (row 1)"), "xml: {xml}");
    assert!(xml.contains("Shop (row 2)"), "xml: {xml}");
    assert!(xml.contains("result=\"Pass\""), "xml: {xml}");
    assert!(xml.contains("result=\"Fail\""), "xml: {xml}");
}

#[test]
fn a_data_file_with_no_rows_exits_two() {
    let scratch = Scratch::new();
    write_workspace(&scratch.0, vec![request("A", "http://127.0.0.1:1/", Vec::new())]);
    let data = scratch.write("empty.csv", "who\n");

    let output = run_ok(ricochet(&["run", scratch.0.to_str().unwrap(), "--data", data.to_str().unwrap()]));

    assert_eq!(exit_code(&output), 2);
    assert!(stderr(&output).contains("has no rows to run"));
}

#[test]
fn a_data_file_with_the_wrong_extension_exits_two() {
    let scratch = Scratch::new();
    write_workspace(&scratch.0, vec![request("A", "http://127.0.0.1:1/", Vec::new())]);
    let data = scratch.write("users.txt", "who\nalice\n");

    let output = run_ok(ricochet(&["run", scratch.0.to_str().unwrap(), "--data", data.to_str().unwrap()]));

    assert_eq!(exit_code(&output), 2);
    assert!(stderr(&output).contains("Use a .csv or .json file."));
}

#[test]
fn a_missing_data_file_exits_two() {
    let scratch = Scratch::new();
    write_workspace(&scratch.0, vec![request("A", "http://127.0.0.1:1/", Vec::new())]);
    let missing = scratch.file("missing.csv");

    let output = run_ok(ricochet(&["run", scratch.0.to_str().unwrap(), "--data", missing.to_str().unwrap()]));

    assert_eq!(exit_code(&output), 2);
    assert!(stderr(&output).contains("Could not read the data file"));
}
