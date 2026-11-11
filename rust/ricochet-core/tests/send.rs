use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use base64::Engine;
use md5::{Digest, Md5};
use pretty_assertions::assert_eq;
use ricochet_core::assertions::{self, RequestAssertion, ops, sources};
use ricochet_core::auth_signing;
use ricochet_core::mock_server::{MockReply, MockRoute, MockRouteTable, MockServer};
use ricochet_core::models::{ApiRequest, AuthDetails, KeyValue, RequestAuth, auth_kinds};
use ricochet_core::send::{DEFAULT_TIMEOUT, RequestSender, SenderSettings, parse_proxy};
use url::Url;

// ---- helpers ----

/// A request the mock server has seen, read off the socket. The mock server does not keep request headers,
/// so tests that check what was sent use this recorder instead. It only listens on 127.0.0.1.
#[derive(Debug, Clone)]
struct Seen {
    request_line: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Seen {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
    }
}

struct Recorder {
    port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Recorder {
    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

/// Serves `count` connections on 127.0.0.1. `respond` gets the connection's index and the request, and returns the
/// raw response, or None to leave the connection open without answering.
fn recorder(count: usize, respond: impl Fn(usize, &Seen) -> Option<String> + Send + 'static) -> Recorder {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    thread::spawn(move || {
        for index in 0..count {
            let Ok((mut stream, _)) = listener.accept() else { return };
            let Some(request) = read_request(&mut stream) else { continue };
            log.lock().unwrap().push(request.clone());
            match respond(index, &request) {
                Some(text) => {
                    let _ = stream.write_all(text.as_bytes());
                }
                None => thread::sleep(Duration::from_secs(3)),
            }
        }
    });
    Recorder { port, seen }
}

fn read_request(stream: &mut TcpStream) -> Option<Seen> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).ok()?;
    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            headers.push((key.trim().to_string(), value.trim().to_string()));
        }
    }
    let length = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).ok()?;
    Some(Seen {
        request_line: request_line.trim_end().to_string(),
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn http(status: u16, reason: &str, headers: &[(&str, &str)], body: &str) -> String {
    let mut text = format!("HTTP/1.1 {status} {reason}\r\n");
    for (key, value) in headers {
        text.push_str(&format!("{key}: {value}\r\n"));
    }
    text.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()));
    text
}

fn ok(body: &str) -> Option<String> {
    Some(http(200, "OK", &[], body))
}

fn hang(_: usize, _: &Seen) -> Option<String> {
    None
}

/// The mock counts a request after it has answered it, so a test that has its response can still be a moment early.
/// Waits for the count to reach `expected`, for up to five seconds, before the test checks it.
fn wait_for_handled(handled: &AtomicUsize, expected: usize) -> usize {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while handled.load(Ordering::SeqCst) < expected && std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    handled.load(Ordering::SeqCst)
}

fn mock(routes: Vec<MockRoute>) -> (MockServer, u16, Arc<AtomicUsize>) {
    let handled = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&handled);
    let mut server = MockServer::new();
    server
        .listen(0, MockRouteTable::new(routes), move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
    let port = server.port().unwrap();
    (server, port, handled)
}

fn route(method: &str, path: &str, status: u16, body: &str, media: Option<&str>, headers: Vec<KeyValue>) -> MockRoute {
    MockRoute::new(
        method,
        path,
        MockReply {
            status_code: status,
            headers,
            body: body.as_bytes().to_vec(),
            media_type: media.map(str::to_string),
        },
    )
}

fn request(method: &str, url: String) -> ApiRequest {
    ApiRequest { method: method.to_string(), url, ..ApiRequest::default() }
}

fn get(url: String) -> ApiRequest {
    request("GET", url)
}

fn local(port: u16, path: &str) -> String {
    format!("http://127.0.0.1:{port}{path}")
}

fn settings_with(change: impl FnOnce(&mut SenderSettings)) -> SenderSettings {
    let mut settings = SenderSettings::default();
    change(&mut settings);
    settings
}

fn scratch_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ricochet-send-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Reads one `name="value"` or `name=value` parameter out of an Authorization header.
fn param(header: &str, name: &str) -> String {
    let marker = format!("{name}=");
    let start = header.find(&marker).expect("parameter present") + marker.len();
    let rest = &header[start..];
    let rest = rest.strip_prefix('"').unwrap_or(rest);
    let end = rest.find(['"', ',']).unwrap_or(rest.len());
    rest[..end].to_string()
}

fn md5_hex(text: &str) -> String {
    Md5::digest(text.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

// ---- sending and the response ----

#[test]
fn a_get_returns_status_headers_and_a_body_the_assertions_can_check() {
    let (_server, port, _) = mock(vec![route(
        "GET",
        "/pets/1",
        200,
        r#"{"name":"Rex"}"#,
        Some("application/json; charset=utf-8"),
        vec![KeyValue::new("X-Trace", "abc")],
    )]);

    let response = RequestSender::new().send(&get(local(port, "/pets/1"))).unwrap();

    assert_eq!(response.status_code, 200);
    assert_eq!(response.media_type.as_deref(), Some("application/json"));
    assert_eq!(response.charset.as_deref(), Some("utf-8"));
    assert_eq!(response.body_text(), r#"{"name":"Rex"}"#);
    assert!(response.headers.iter().any(|h| h.key.eq_ignore_ascii_case("x-trace") && h.value == "abc"));
    assert!(!response.truncated);
    assert!(!response.certificates_unverified);
    assert!(response.elapsed < DEFAULT_TIMEOUT);

    let name_is_rex = RequestAssertion {
        source: sources::JSON.to_string(),
        path: "name".to_string(),
        op: ops::EQUAL.to_string(),
        expected: "Rex".to_string(),
        ..RequestAssertion::default()
    };
    let status_is_201 = RequestAssertion {
        source: sources::STATUS.to_string(),
        op: ops::EQUAL.to_string(),
        expected: "201".to_string(),
        ..RequestAssertion::default()
    };
    let results = assertions::evaluate(&[name_is_rex, status_is_201], &response.snapshot());
    assert_eq!(results.iter().map(|r| r.passed).collect::<Vec<_>>(), vec![true, false]);
}

#[test]
fn a_url_without_a_scheme_is_sent_as_http() {
    let (_server, port, handled) = mock(vec![route("GET", "/path", 200, "ok", None, vec![])]);

    let response = RequestSender::new().send(&get(format!("127.0.0.1:{port}/path"))).unwrap();

    assert_eq!(response.status_code, 200);
    assert_eq!(wait_for_handled(&handled, 1), 1);
}

#[test]
fn an_empty_url_and_a_non_http_url_are_refused_before_anything_is_sent() {
    let sender = RequestSender::new();

    assert_eq!(sender.send(&get(String::new())).unwrap_err().message(), "Enter a URL.");
    assert_eq!(
        sender.send(&get("ftp://127.0.0.1/x".to_string())).unwrap_err().message(),
        "Invalid URL: ftp://127.0.0.1/x"
    );
}

#[test]
fn an_invalid_method_is_refused_and_nothing_is_sent() {
    let (_server, port, handled) = mock(vec![route("GET", "/", 200, "ok", None, vec![])]);

    let error = RequestSender::new().send(&request("BAD METHOD", local(port, "/"))).unwrap_err();

    assert_eq!(error.message(), "Invalid method: BAD METHOD.");
    assert_eq!(handled.load(Ordering::SeqCst), 0);
}

#[test]
fn a_timeout_reports_the_limit_in_seconds() {
    let slow = recorder(1, hang);
    let mut sender = RequestSender::new();
    sender.configure(settings_with(|s| s.timeout = Duration::from_secs(1)));

    let error = sender.send(&get(format!("http://127.0.0.1:{}/slow", slow.port))).unwrap_err();

    assert_eq!(error.message(), "Timed out after 1 seconds.");
}

#[test]
fn redirects_are_followed_unless_turned_off() {
    let (_server, port, _) = mock(vec![
        route("GET", "/start", 302, "", None, vec![KeyValue::new("Location", "/end")]),
        route("GET", "/end", 200, "done", Some("text/plain"), vec![]),
    ]);
    let mut sender = RequestSender::new();

    let followed = sender.send(&get(local(port, "/start"))).unwrap();
    assert_eq!(followed.status_code, 200);
    assert_eq!(followed.body_text(), "done");

    assert_eq!(sender.configure(settings_with(|s| s.follow_redirects = false)), None);
    let not_followed = sender.send(&get(local(port, "/start"))).unwrap();
    assert_eq!(not_followed.status_code, 302);
    assert!(not_followed.headers.iter().any(|h| h.key.eq_ignore_ascii_case("location") && h.value == "/end"));
}

#[test]
fn a_body_over_the_limit_is_cut_and_marked_truncated() {
    let (_server, port, _) = mock(vec![route("GET", "/big", 200, &"x".repeat(3000), Some("text/plain"), vec![])]);
    let mut sender = RequestSender::new();
    sender.configure(settings_with(|s| s.max_body_bytes = 1000));

    let response = sender.send(&get(local(port, "/big"))).unwrap();

    assert_eq!(response.body.len(), 1000);
    assert!(response.truncated);
}

#[test]
fn a_body_of_exactly_the_limit_is_not_truncated() {
    let (_server, port, _) = mock(vec![route("GET", "/exact", 200, &"y".repeat(1000), Some("text/plain"), vec![])]);
    let mut sender = RequestSender::new();
    sender.configure(settings_with(|s| s.max_body_bytes = 1000));

    let response = sender.send(&get(local(port, "/exact"))).unwrap();

    assert_eq!(response.body.len(), 1000);
    assert!(!response.truncated);
}

// ---- request building ----

#[test]
fn disabled_and_blank_headers_are_not_sent() {
    let recorded = recorder(1, |_, _| ok("ok"));
    let mut sent = get(format!("http://127.0.0.1:{}/", recorded.port));
    sent.headers = vec![
        KeyValue::new("X-On", "1"),
        KeyValue { enabled: false, ..KeyValue::new("X-Off", "2") },
        KeyValue::new("   ", "3"),
    ];

    RequestSender::new().send(&sent).unwrap();

    let seen = &recorded.seen()[0];
    assert_eq!(seen.header("X-On"), Some("1"));
    assert_eq!(seen.header("X-Off"), None);
    assert!(seen.headers.iter().all(|(key, _)| !key.is_empty()));
}

#[test]
fn a_tab_in_a_header_value_is_allowed() {
    let recorded = recorder(1, |_, _| ok("ok"));
    let mut sent = get(format!("http://127.0.0.1:{}/", recorded.port));
    sent.headers = vec![KeyValue::new("X-Tabbed", "a\tb")];

    let response = RequestSender::new().send(&sent).unwrap();

    assert_eq!(response.status_code, 200);
    assert_eq!(recorded.seen()[0].header("X-Tabbed"), Some("a\tb"));
}

#[test]
fn a_body_without_a_content_type_is_sent_as_json_or_text_by_its_first_character() {
    let recorded = recorder(2, |_, _| ok("ok"));
    let sender = RequestSender::new();

    let mut json = request("POST", format!("http://127.0.0.1:{}/", recorded.port));
    json.body = "  {\"a\": 1}".to_string();
    sender.send(&json).unwrap();

    let mut text = request("POST", format!("http://127.0.0.1:{}/", recorded.port));
    text.body = "plain words".to_string();
    sender.send(&text).unwrap();

    let seen = recorded.seen();
    assert_eq!(seen[0].header("Content-Type"), Some("application/json"));
    assert_eq!(seen[0].body, "  {\"a\": 1}");
    assert_eq!(seen[1].header("Content-Type"), Some("text/plain; charset=utf-8"));
}

#[test]
fn a_content_type_written_on_the_request_wins_over_the_guess() {
    let recorded = recorder(1, |_, _| ok("ok"));
    let mut sent = request("POST", format!("http://127.0.0.1:{}/", recorded.port));
    sent.body = "{\"a\": 1}".to_string();
    sent.headers = vec![KeyValue::new("content-type", "text/csv")];

    RequestSender::new().send(&sent).unwrap();

    assert_eq!(recorded.seen()[0].header("Content-Type"), Some("text/csv"));
}

#[test]
fn a_content_header_without_a_body_is_dropped() {
    let recorded = recorder(1, |_, _| ok("ok"));
    let mut sent = get(format!("http://127.0.0.1:{}/", recorded.port));
    sent.headers = vec![KeyValue::new("Content-Type", "application/json")];

    RequestSender::new().send(&sent).unwrap();

    assert_eq!(recorded.seen()[0].header("Content-Type"), None);
}

#[test]
fn a_control_character_in_the_url_is_refused_and_nothing_is_sent() {
    let sender = RequestSender::new();
    for control in ["\n", "\r", "\0", "\x1b"] {
        let (_server, port, handled) = mock(vec![route("GET", "/a", 200, "ok", None, vec![])]);

        let error = sender.send(&get(format!("http://127.0.0.1:{port}/a{control}b"))).unwrap_err();

        assert!(error.message().contains("URL"), "{}", error.message());
        assert_eq!(handled.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn a_line_break_in_a_header_value_is_refused_and_names_the_header() {
    let (_server, port, handled) = mock(vec![route("GET", "/", 200, "ok", None, vec![])]);
    let mut sent = get(local(port, "/"));
    sent.headers = vec![KeyValue::new("X-Trace", "abc\r\nX-Injected: yes")];

    let error = RequestSender::new().send(&sent).unwrap_err();

    assert!(error.message().contains("X-Trace"), "{}", error.message());
    assert_eq!(handled.load(Ordering::SeqCst), 0);
}

#[test]
fn a_control_character_in_a_header_name_is_refused() {
    let (_server, port, handled) = mock(vec![route("GET", "/", 200, "ok", None, vec![])]);
    let mut sent = get(local(port, "/"));
    sent.headers = vec![KeyValue::new("X-Bad\nName", "value")];

    assert!(RequestSender::new().send(&sent).is_err());
    assert_eq!(handled.load(Ordering::SeqCst), 0);
}

#[test]
fn a_line_break_in_a_json_body_is_still_allowed() {
    let (_server, port, _) = mock(vec![route("POST", "/", 200, "ok", None, vec![])]);
    let mut sent = request("POST", local(port, "/"));
    sent.body = "{\n  \"name\": \"one\"\n}".to_string();

    assert_eq!(RequestSender::new().send(&sent).unwrap().status_code, 200);
}

// ---- auth ----

#[test]
fn bearer_auth_is_sent_as_written() {
    let recorded = recorder(1, |_, _| ok("ok"));
    let mut sent = get(format!("http://127.0.0.1:{}/", recorded.port));
    sent.auth = Some(RequestAuth { kind: auth_kinds::BEARER.to_string(), token: "abc def".to_string(), ..RequestAuth::default() });

    RequestSender::new().send(&sent).unwrap();

    assert_eq!(recorded.seen()[0].header("Authorization"), Some("Bearer abc def"));
}

#[test]
fn a_line_break_in_a_bearer_token_is_refused_and_never_sent() {
    let (_server, port, handled) = mock(vec![route("GET", "/", 200, "ok", None, vec![])]);
    let mut sent = get(local(port, "/"));
    sent.auth = Some(RequestAuth {
        kind: auth_kinds::BEARER.to_string(),
        token: "abc\nX-Injected: yes".to_string(),
        ..RequestAuth::default()
    });

    let error = RequestSender::new().send(&sent).unwrap_err();

    assert!(error.message().contains("Authorization"), "{}", error.message());
    assert_eq!(handled.load(Ordering::SeqCst), 0);
}

#[test]
fn basic_auth_sends_the_user_and_password_encoded() {
    let recorded = recorder(1, |_, _| ok("ok"));
    let mut sent = get(format!("http://127.0.0.1:{}/", recorded.port));
    sent.auth = Some(RequestAuth {
        kind: auth_kinds::BASIC.to_string(),
        username: "Mufasa".to_string(),
        password: "Circle Of Life".to_string(),
        ..RequestAuth::default()
    });

    RequestSender::new().send(&sent).unwrap();

    let expected = format!("Basic {}", base64::engine::general_purpose::STANDARD.encode("Mufasa:Circle Of Life"));
    assert_eq!(recorded.seen()[0].header("Authorization"), Some(expected.as_str()));
}

#[test]
fn an_api_key_goes_in_a_header_or_in_the_query_escaped() {
    let recorded = recorder(2, |_, _| ok("ok"));
    let sender = RequestSender::new();

    let mut in_header = get(format!("http://127.0.0.1:{}/", recorded.port));
    in_header.auth = Some(RequestAuth {
        kind: auth_kinds::API_KEY.to_string(),
        key_name: "X-Api-Key".to_string(),
        key_value: "secret".to_string(),
        ..RequestAuth::default()
    });
    sender.send(&in_header).unwrap();

    let mut in_query = get(format!("http://127.0.0.1:{}/search?q=1", recorded.port));
    in_query.auth = Some(RequestAuth {
        kind: auth_kinds::API_KEY.to_string(),
        key_name: "api key".to_string(),
        key_value: "a b&c".to_string(),
        key_in_query: true,
        ..RequestAuth::default()
    });
    sender.send(&in_query).unwrap();

    let seen = recorded.seen();
    assert_eq!(seen[0].header("X-Api-Key"), Some("secret"));
    assert_eq!(seen[1].request_line, "GET /search?q=1&api%20key=a%20b%26c HTTP/1.1");
}

#[test]
fn a_control_character_in_an_api_key_sent_in_the_query_is_refused() {
    let (_server, port, handled) = mock(vec![route("GET", "/", 200, "ok", None, vec![])]);
    let mut sent = get(local(port, "/"));
    sent.auth = Some(RequestAuth {
        kind: auth_kinds::API_KEY.to_string(),
        key_name: "key".to_string(),
        key_value: "a\rb".to_string(),
        key_in_query: true,
        ..RequestAuth::default()
    });

    let error = RequestSender::new().send(&sent).unwrap_err();

    assert!(error.message().contains("key"), "{}", error.message());
    assert_eq!(handled.load(Ordering::SeqCst), 0);
}

#[test]
fn a_header_written_by_hand_wins_over_the_auth_settings() {
    let recorded = recorder(1, |_, _| ok("ok"));
    let mut sent = get(format!("http://127.0.0.1:{}/", recorded.port));
    sent.headers = vec![KeyValue::new("Authorization", "Custom")];
    sent.auth = Some(RequestAuth { kind: auth_kinds::BEARER.to_string(), token: "t".to_string(), ..RequestAuth::default() });

    RequestSender::new().send(&sent).unwrap();

    assert_eq!(recorded.seen()[0].header("Authorization"), Some("Custom"));
}

#[test]
fn a_digest_challenge_is_answered_once_with_a_response_the_server_can_check() {
    let realm = "testrealm@host.com";
    let nonce = "dcd98b7102dd2f0e8b11d0f600bfb0c093";
    let recorded = recorder(2, move |index, _| {
        if index == 0 {
            let challenge = format!(r#"Digest realm="{realm}", qop="auth", nonce="{nonce}", opaque="5ccc069c403ebaf9f0171e9517f40e41""#);
            Some(http(401, "Unauthorized", &[("WWW-Authenticate", challenge.as_str())], ""))
        } else {
            ok("ok")
        }
    });
    let mut sent = get(format!("http://127.0.0.1:{}/dir/index.html?a=1", recorded.port));
    sent.auth = Some(RequestAuth {
        kind: auth_kinds::DIGEST.to_string(),
        username: "Mufasa".to_string(),
        password: "Circle Of Life".to_string(),
        ..RequestAuth::default()
    });

    let response = RequestSender::new().send(&sent).unwrap();

    assert_eq!(response.status_code, 200);
    let seen = recorded.seen();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].header("Authorization"), None);

    let answer = seen[1].header("Authorization").expect("retry carries Authorization");
    assert!(answer.starts_with("Digest "), "{answer}");
    assert_eq!(param(answer, "username"), "Mufasa");
    assert_eq!(param(answer, "uri"), "/dir/index.html?a=1");
    assert_eq!(param(answer, "algorithm"), "MD5");
    assert_eq!(param(answer, "qop"), "auth");
    assert_eq!(param(answer, "nc"), "00000001");
    let cnonce = param(answer, "cnonce");
    assert_eq!(cnonce.len(), 32);

    // RFC 7616 with MD5 and qop auth, worked out here rather than by the crate under test.
    let ha1 = md5_hex(&format!("Mufasa:{realm}:Circle Of Life"));
    let ha2 = md5_hex("GET:/dir/index.html?a=1");
    assert_eq!(param(answer, "response"), md5_hex(&format!("{ha1}:{nonce}:00000001:{cnonce}:auth:{ha2}")));
}

#[test]
fn a_digest_challenge_with_an_unknown_algorithm_stops_after_the_401() {
    let recorded = recorder(2, |_, _| {
        Some(http(401, "Unauthorized", &[("WWW-Authenticate", r#"Digest realm="r", nonce="n", algorithm=SHA-512-256"#)], ""))
    });
    let mut sent = get(format!("http://127.0.0.1:{}/", recorded.port));
    sent.auth = Some(RequestAuth {
        kind: auth_kinds::DIGEST.to_string(),
        username: "u".to_string(),
        password: "p".to_string(),
        ..RequestAuth::default()
    });

    let error = RequestSender::new().send(&sent).unwrap_err();

    assert!(error.message().contains("SHA-512-256"), "{}", error.message());
    assert_eq!(recorded.seen().len(), 1);
}

#[test]
fn oauth1_signs_form_fields_but_not_a_json_body() {
    let recorded = recorder(2, |_, _| ok("ok"));
    let auth = RequestAuth {
        kind: auth_kinds::OAUTH1.to_string(),
        token: "token".to_string(),
        details: Some(AuthDetails {
            consumer_key: "consumer".to_string(),
            consumer_secret: "consumer-secret".to_string(),
            token_secret: "token-secret".to_string(),
            ..AuthDetails::default()
        }),
        ..RequestAuth::default()
    };
    let url = format!("http://127.0.0.1:{}/a?include=1", recorded.port);
    let sender = RequestSender::new();

    let mut form = request("POST", url.clone());
    form.auth = Some(auth.clone());
    form.body = "status=Hello+World".to_string();
    form.headers = vec![KeyValue::new("Content-Type", "application/x-www-form-urlencoded")];
    sender.send(&form).unwrap();

    let mut json = request("POST", url.clone());
    json.auth = Some(auth.clone());
    json.body = r#"{"status":"Hello World"}"#.to_string();
    json.headers = vec![KeyValue::new("Content-Type", "application/json")];
    sender.send(&json).unwrap();

    let seen = recorded.seen();
    let parsed_url = Url::parse(&url).unwrap();
    for (sent, fields) in [(&seen[0], vec![("status".to_string(), "Hello World".to_string())]), (&seen[1], Vec::new())] {
        let header = sent.header("Authorization").expect("signed");
        let nonce = param(header, "oauth_nonce");
        let timestamp: i64 = param(header, "oauth_timestamp").parse().unwrap();
        let expected = auth_signing::oauth1_authorization(&auth, "POST", &parsed_url, &fields, &nonce, timestamp).unwrap();
        assert_eq!(header, expected);
    }
}

#[test]
fn aws_sig_v4_signs_host_and_date_and_sends_the_session_token() {
    let recorded = recorder(1, |_, _| ok("ok"));
    let mut sent = get(format!("http://127.0.0.1:{}/", recorded.port));
    sent.auth = Some(RequestAuth {
        kind: auth_kinds::AWS_SIG_V4.to_string(),
        details: Some(AuthDetails {
            aws_access_key_id: "AKIDEXAMPLE".to_string(),
            aws_secret_access_key: "secret".to_string(),
            aws_session_token: "session-token-value".to_string(),
            aws_region: "us-east-1".to_string(),
            aws_service: "execute-api".to_string(),
            ..AuthDetails::default()
        }),
        ..RequestAuth::default()
    });

    RequestSender::new().send(&sent).unwrap();

    let seen = &recorded.seen()[0];
    assert_eq!(seen.header("X-Amz-Security-Token"), Some("session-token-value"));
    assert_eq!(seen.header("X-Amz-Date").map(str::len), Some(16));
    let authorization = seen.header("Authorization").unwrap();
    assert!(authorization.starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/"), "{authorization}");
    assert!(authorization.contains("SignedHeaders=host;x-amz-date;x-amz-security-token, "), "{authorization}");
}

// ---- proxy and certificates ----

#[test]
fn proxy_addresses_default_to_http_and_refuse_what_cannot_be_used() {
    assert_eq!(parse_proxy("proxy.corp:8080").unwrap().map(|u| u.to_string()), Some("http://proxy.corp:8080/".to_string()));
    assert_eq!(parse_proxy("   ").unwrap(), None);
    assert!(parse_proxy("socks5://proxy.corp:1080").unwrap_err().message().starts_with("Invalid proxy address"));
    assert!(parse_proxy("http://user:pass@proxy.corp:8080")
        .unwrap_err()
        .message()
        .contains("Proxy logins are not supported"));
}

#[test]
fn a_proxy_receives_the_request_when_one_is_set() {
    let (_server, proxy_port, handled) = mock(vec![route("GET", "/hello", 200, "via proxy", None, vec![])]);
    let mut sender = RequestSender::new();
    assert_eq!(sender.configure(settings_with(|s| s.proxy_url = format!("127.0.0.1:{proxy_port}"))), None);

    // example.invalid is never resolved: the request goes to the proxy, which answers it.
    let response = sender.send(&get("http://example.invalid/hello".to_string())).unwrap();

    assert_eq!(response.body_text(), "via proxy");
    assert_eq!(wait_for_handled(&handled, 1), 1);
}

#[test]
fn an_invalid_proxy_is_reported_by_configure_and_stops_every_send() {
    let (_server, port, handled) = mock(vec![route("GET", "/", 200, "ok", None, vec![])]);
    let mut sender = RequestSender::new();

    let problem = sender.configure(settings_with(|s| s.proxy_url = "socks5://proxy.corp:1080".to_string())).unwrap();
    assert!(problem.starts_with("Invalid proxy address"), "{problem}");

    let error = sender.send(&get(local(port, "/"))).unwrap_err();
    assert_eq!(error.message(), problem);
    assert_eq!(handled.load(Ordering::SeqCst), 0);
}

#[test]
fn a_missing_client_certificate_stops_requests_with_a_clear_message() {
    let dir = scratch_dir();
    let mut sender = RequestSender::new();
    let missing = dir.join("missing.pem").to_string_lossy().into_owned();

    let problem = sender.configure(settings_with(|s| s.client_certificate_path = missing)).unwrap();

    assert!(problem.contains("was not found"), "{problem}");
    assert!(sender.send(&get("http://127.0.0.1:9/".to_string())).unwrap_err().message().contains("was not found"));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_pkcs12_client_certificate_is_refused_with_the_reason() {
    let dir = scratch_dir();
    let pfx = dir.join("client.pfx");
    fs::write(&pfx, b"not a real certificate").unwrap();
    let mut sender = RequestSender::new();

    let problem = sender
        .configure(settings_with(|s| s.client_certificate_path = pfx.to_string_lossy().into_owned()))
        .unwrap();

    assert!(problem.contains("not supported yet"), "{problem}");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn an_unreadable_pem_client_certificate_is_reported() {
    let dir = scratch_dir();
    let pem = dir.join("client.pem");
    fs::write(&pem, b"garbage, not PEM").unwrap();
    let mut sender = RequestSender::new();

    let problem = sender.configure(settings_with(|s| s.client_certificate_path = pem.to_string_lossy().into_owned())).unwrap();

    assert!(problem.contains("could not be loaded"), "{problem}");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn certificate_checks_are_on_by_default_and_warn_when_turned_off() {
    let defaults = SenderSettings::default();
    assert!(defaults.verify_certificates);
    assert_eq!(defaults.certificate_warning(), None);

    let (_server, port, _) = mock(vec![route("GET", "/", 200, "ok", None, vec![])]);
    let checked = RequestSender::new().send(&get(local(port, "/"))).unwrap();
    assert!(!checked.certificates_unverified);

    let mut unchecked_sender = RequestSender::new();
    assert_eq!(unchecked_sender.configure(settings_with(|s| s.verify_certificates = false)), None);
    let warning = unchecked_sender.settings().certificate_warning().expect("warning while checks are off");
    assert!(warning.starts_with("Certificate checks are off"), "{warning}");

    let unchecked = unchecked_sender.send(&get(local(port, "/"))).unwrap();
    assert!(unchecked.certificates_unverified);
}
