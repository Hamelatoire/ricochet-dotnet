//! Ports the cookie cases of `tests/Ricochet.Core.Tests/RunCookiesTests.cs` (ADR 0012): a run that keeps cookies sends
//! back what the server set, a run that doesn't sends none, each run starts empty, and a Request with its own Cookie
//! header is sent without the stored ones. A small loopback server sets a cookie on /login and echoes the Cookie
//! header it receives on every other path.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::thread;

use ricochet_core::assertions::{RequestAssertion, ops, sources};
use ricochet_core::collection_runner::{CollectionRunner, RunOptions};
use ricochet_core::models::{ApiRequest, Collection, CollectionItem, KeyValue, Workspace};
use ricochet_core::send::RequestSender;

/// Starts the server and returns its base URL. It answers `count` connections, then stops.
fn cookie_site(count: usize) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming().take(count) {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            let mut cookie = String::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':') {
                    if name.eq_ignore_ascii_case("cookie") {
                        cookie = value.trim().to_string();
                    }
                }
            }
            let set = if first.contains(" /login ") { "Set-Cookie: session=abc123; Path=/\r\n" } else { "" };
            let body = format!("cookie=[{cookie}]");
            let reply = format!(
                "HTTP/1.1 200 OK\r\n{set}Content-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).unwrap();
        }
    });
    format!("http://127.0.0.1:{port}")
}

fn request(url: &str) -> ApiRequest {
    ApiRequest { method: "GET".to_string(), url: url.to_string(), ..ApiRequest::default() }
}

fn body_contains(text: &str) -> RequestAssertion {
    RequestAssertion {
        source: sources::BODY.to_string(),
        op: ops::CONTAINS.to_string(),
        expected: text.to_string(),
        ..RequestAssertion::default()
    }
}

fn login_then_check(base: &str, expected: &str) -> Workspace {
    let mut check = request(&format!("{base}/me"));
    check.assertions = Some(vec![body_contains(expected)]);
    let items = vec![
        CollectionItem { id: "login".into(), name: "login".into(), request: Some(request(&format!("{base}/login"))), ..CollectionItem::default() },
        CollectionItem { id: "me".into(), name: "me".into(), request: Some(check), ..CollectionItem::default() },
    ];
    Workspace { collections: vec![Collection { name: "c".into(), items, ..Collection::default() }], ..Workspace::default() }
}

#[test]
fn a_run_that_keeps_cookies_sends_back_what_the_server_set() {
    let base = cookie_site(2);
    let sender = RequestSender::new();
    let options = RunOptions { keep_cookies: true, ..RunOptions::default() };
    let report = CollectionRunner::new(&sender).run(&login_then_check(&base, "cookie=[session=abc123]"), &options).unwrap();
    assert!(report.passed(), "{:?}", report.results);
}

#[test]
fn a_run_without_cookies_sends_none() {
    let base = cookie_site(2);
    let sender = RequestSender::new();
    let report = CollectionRunner::new(&sender).run(&login_then_check(&base, "cookie=[]"), &RunOptions::default()).unwrap();
    assert!(report.passed(), "{:?}", report.results);
}

#[test]
fn each_run_starts_with_no_cookies() {
    let base = cookie_site(3);
    let sender = RequestSender::new();
    let first = sender.begin_run_cookies();
    sender.send_in_run(&request(&format!("{base}/login")), &first).unwrap();
    drop(first);

    let second = sender.begin_run_cookies();
    let response = sender.send_in_run(&request(&format!("{base}/me")), &second).unwrap();
    assert_eq!(response.body_text(), "cookie=[]");

    // The sender's own sends never keep cookies.
    let plain = sender.send(&request(&format!("{base}/me"))).unwrap();
    assert_eq!(plain.body_text(), "cookie=[]");
}

#[test]
fn a_request_with_its_own_cookie_header_is_sent_without_the_stored_ones() {
    let base = cookie_site(2);
    let sender = RequestSender::new();
    let run = sender.begin_run_cookies();
    sender.send_in_run(&request(&format!("{base}/login")), &run).unwrap();

    let mut own = request(&format!("{base}/me"));
    own.headers = vec![KeyValue::new("Cookie", "mine=1")];
    let response = sender.send_in_run(&own, &run).unwrap();
    assert_eq!(response.body_text(), "cookie=[mine=1]");
}
