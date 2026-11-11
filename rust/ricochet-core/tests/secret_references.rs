//! Ports the Secret reference cases (ADR 0025) of `VariableResolverTests.cs`, `CollectionRunnerTests.cs` and
//! `BulkRunnerTests.cs`: references are read only when the send needs them, through other Variables too, their
//! values are masked, a prompt Variable is never read, and an unreadable reference stops the send. A loopback server
//! echoes each request's head as its body, so the tests can see what was sent.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;

use ricochet_core::assertions::{RequestAssertion, ops, sources};
use ricochet_core::bulk_runner::{BulkRun, BulkRunner};
use ricochet_core::collection_runner::{CollectionRunner, RunOptions};
use ricochet_core::external_secrets::{CommandOutput, CommandRunner, ExternalSecretError, ExternalSecretResolver, reference};
use ricochet_core::models::{ApiRequest, Collection, CollectionItem, KeyValue, Workspace};
use ricochet_core::send::RequestSender;
use ricochet_core::variables;

const TOKEN: &str = "s3cret-token-value";

/// Counts the commands it is asked to run, and never runs one.
#[derive(Clone, Default)]
struct NoCommands(Arc<Mutex<Vec<String>>>);

impl CommandRunner for NoCommands {
    fn run(&self, command_line: &str) -> Result<CommandOutput, ExternalSecretError> {
        self.0.lock().unwrap().push(command_line.to_string());
        Err(ExternalSecretError::new("commands are not run in tests"))
    }
}

/// Reads only `API_TOKEN` from a fake environment.
fn resolver(commands: NoCommands) -> ExternalSecretResolver {
    ExternalSecretResolver::new(|name| (name == "API_TOKEN").then(|| TOKEN.to_string()), commands)
}

fn echo_site(count: usize) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming().take(count) {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = String::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
                head.push_str(&line);
            }
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{head}",
                head.len()
            );
            stream.write_all(reply.as_bytes()).unwrap();
        }
    });
    format!("http://127.0.0.1:{port}")
}

fn bearer_request(url: &str) -> ApiRequest {
    ApiRequest {
        method: "GET".into(),
        url: url.into(),
        headers: vec![KeyValue::new("Authorization", "Bearer {{auth}}")],
        ..ApiRequest::default()
    }
}

fn body_check(op: &str, text: &str) -> RequestAssertion {
    RequestAssertion { source: sources::BODY.into(), op: op.into(), expected: text.into(), ..RequestAssertion::default() }
}

/// `auth` points at `token`, which is a reference, so the reference is found through another Variable.
fn token_variables(token_reference: &str) -> Vec<KeyValue> {
    vec![KeyValue::new("auth", "{{token}}"), KeyValue::new("token", token_reference)]
}

fn workspace(request: ApiRequest, variables: Vec<KeyValue>) -> Workspace {
    let item = CollectionItem { id: "r".into(), name: "r".into(), request: Some(request), ..CollectionItem::default() };
    Workspace { collections: vec![Collection { name: "c".into(), items: vec![item], variables, ..Collection::default() }], ..Workspace::default() }
}

#[test]
fn a_reference_the_request_uses_is_read_and_marked_as_a_secret() {
    let variables = token_variables(&reference::for_environment_variable("API_TOKEN"));
    let resolved = variables::resolve_references(&bearer_request("http://x"), &variables, &resolver(NoCommands::default())).unwrap();

    let token = resolved.iter().rev().find(|v| v.key == "token").unwrap();
    assert_eq!(token.value, TOKEN);
    assert!(token.is_secret);
    let sent = variables::apply(&bearer_request("http://x"), &resolved);
    assert_eq!(sent.headers[0].value, format!("Bearer {TOKEN}"));
}

#[test]
fn a_reference_the_request_does_not_use_is_never_read() {
    let commands = NoCommands::default();
    let variables = vec![KeyValue::new("unused", &reference::for_command("vault read x"))];
    let request = ApiRequest { method: "GET".into(), url: "http://x".into(), ..ApiRequest::default() };
    let resolved = variables::resolve_references(&request, &variables, &resolver(commands.clone())).unwrap();

    assert_eq!(resolved, variables);
    assert!(commands.0.lock().unwrap().is_empty());
}

#[test]
fn a_prompt_variable_is_never_read() {
    let commands = NoCommands::default();
    let mut prompt = KeyValue::new("auth", &reference::for_command("vault read x"));
    prompt.is_prompt = true;
    let resolved = variables::resolve_references(&bearer_request("http://x"), &[prompt], &resolver(commands.clone())).unwrap();

    assert_eq!(resolved.len(), 1);
    assert!(commands.0.lock().unwrap().is_empty());
}

#[test]
fn an_unreadable_reference_names_the_variable() {
    let variables = token_variables(&reference::for_environment_variable("MISSING"));
    let error = variables::resolve_references(&bearer_request("http://x"), &variables, &resolver(NoCommands::default())).unwrap_err();
    assert!(error.starts_with("The secret reference for token could not be read."), "{error}");
}

#[test]
fn a_run_sends_the_reference_value_and_masks_it_in_failures() {
    let base = echo_site(1);
    let mut request = bearer_request(&format!("{base}/me"));
    // Fails on purpose, so the failure message shows the body that holds the value.
    request.assertions = Some(vec![body_check(ops::NOT_CONTAINS, &format!("Bearer {TOKEN}"))]);
    let ws = workspace(request, token_variables(&reference::for_environment_variable("API_TOKEN")));

    let sender = RequestSender::new();
    let report = CollectionRunner::with_secrets(&sender, resolver(NoCommands::default())).run(&ws, &RunOptions::default()).unwrap();

    let result = &report.results[0];
    assert_eq!(result.status_code, Some(200), "{:?}", result.error);
    assert!(!result.assertions[0].passed, "the value was sent, so the check fails");
    assert!(!result.assertions[0].message.contains(TOKEN), "{}", result.assertions[0].message);
    assert!(!result.assertions[0].description.contains(TOKEN));
}

#[test]
fn a_run_does_not_send_a_request_whose_reference_cannot_be_read() {
    let ws = workspace(bearer_request("http://127.0.0.1:9/never"), token_variables(&reference::for_environment_variable("MISSING")));
    let sender = RequestSender::new();
    let report = CollectionRunner::with_secrets(&sender, resolver(NoCommands::default())).run(&ws, &RunOptions::default()).unwrap();

    let result = &report.results[0];
    assert_eq!(result.status_code, None);
    assert!(result.error.as_deref().unwrap().contains("The secret reference for token could not be read."));
}

#[test]
fn a_bulk_run_reads_the_reference_for_each_id_and_stops_only_that_id_when_it_cannot() {
    let base = echo_site(1);
    let template = bearer_request(&format!("{base}/items/{{{{id}}}}"));
    let collection = Collection { name: "c".into(), variables: token_variables(&reference::for_environment_variable("API_TOKEN")), ..Collection::default() };
    let ws = Workspace::default();
    let ids = vec!["1".to_string()];
    let run = BulkRun {
        workspace: &ws,
        collection: &collection,
        item_id: None,
        environment: None,
        template: &template,
        placeholder: "id",
        ids: &ids,
        overrides: &[],
        max_parallel: 1,
    };
    let sender = RequestSender::new();
    let results = Mutex::new(Vec::new());
    BulkRunner::with_secrets(&sender, resolver(NoCommands::default()))
        .run(&run, &AtomicBool::new(false), |result| results.lock().unwrap().push(result))
        .unwrap();
    let ok = results.into_inner().unwrap();
    assert!(ok[0].succeeded, "{}", ok[0].detail);

    let broken = Collection { variables: token_variables(&reference::for_environment_variable("MISSING")), ..collection.clone() };
    let results = Mutex::new(Vec::new());
    BulkRunner::with_secrets(&sender, resolver(NoCommands::default()))
        .run(&BulkRun { collection: &broken, ..run }, &AtomicBool::new(false), |result| results.lock().unwrap().push(result))
        .unwrap();
    let failed = results.into_inner().unwrap();
    assert!(!failed[0].succeeded);
    assert_eq!(failed[0].status_code, None);
    assert!(failed[0].detail.contains("could not be read"), "{}", failed[0].detail);
}
