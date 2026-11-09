//! Port of src/Ricochet.Core/MockServer.cs (ADR 0027). Builds routes from saved examples and from OpenAPI 3
//! JSON descriptions, matches a request to a route, and runs a local server between `start` and `stop`.
//!
//! The server listens on 127.0.0.1 only, so nothing else on the network can reach it. It opens no outgoing
//! connections and logs only the path and status of each answered request, never the query string or headers.
//! The routing and route building are pure, so they are tested without sockets. The server is plain std
//! networking (HTTP/1.1, one request per connection, `Connection: close`), not an HTTP crate: see [`MockServer`].

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use chrono::{DateTime, Local};
use regex::Regex;
use serde_json::{json, Map, Value};

use crate::models::KeyValue;

/// The lowest port the app offers. Ports below it need privileges and are refused by `MockServer::start`.
pub const MIN_PORT: u16 = 1024;
/// The highest port the app offers.
pub const MAX_PORT: u16 = 65535;

/// Deepest level of nested schema or `$ref` that a sample follows before it gives up and writes null.
const MAX_SAMPLE_DEPTH: usize = 8;

/// The HTTP methods an OpenAPI path item can hold, in the order the description lists them.
const OPENAPI_METHODS: [&str; 8] = ["get", "put", "post", "delete", "options", "head", "patch", "trace"];

/// Headers the server sets itself. A mock's own copy is skipped so the reply stays well formed.
const RESERVED_HEADERS: [&str; 3] = ["Content-Length", "Transfer-Encoding", "Connection"];

/// How often the accept loop checks the stop flag while no request is waiting.
const POLL_INTERVAL: Duration = Duration::from_millis(10);
/// How long one connection may stall on a read or write before it is dropped.
const IO_TIMEOUT: Duration = Duration::from_secs(2);
/// The most request-head bytes read before the request is refused.
const MAX_HEAD_BYTES: usize = 64 * 1024;
/// The most request-body bytes read and thrown away, so the client gets its reply rather than a reset.
const MAX_DRAIN_BYTES: u64 = 1024 * 1024;

/// A problem that stops the mock server from starting, or stops a description from being read.
/// The message is suitable for showing to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockServerError {
    message: String,
}

impl MockServerError {
    fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for MockServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for MockServerError {}

/// One response a mock can give: a method, a URL (or path) and the reply to send.
/// Saved examples (ADR 0023) become these, and so do operations from an OpenAPI description (ADR 0027).
#[derive(Debug, Clone, PartialEq)]
pub struct MockExample {
    pub method: String,
    pub url: String,
    pub status_code: u16,
    pub media_type: Option<String>,
    pub body: String,
    pub headers: Vec<KeyValue>,
}

/// The reply sent for a matched request.
#[derive(Debug, Clone, PartialEq)]
pub struct MockReply {
    pub status_code: u16,
    pub headers: Vec<KeyValue>,
    pub body: Vec<u8>,
    pub media_type: Option<String>,
}

/// A request that was answered, as shown in the log. It holds the path only, never the query string or
/// headers, which can carry secrets.
#[derive(Debug, Clone, PartialEq)]
pub struct MockLogEntry {
    pub at: DateTime<Local>,
    pub method: String,
    pub path: String,
    pub status_code: u16,
}

/// A method and path that a mock answers. Segments in braces (`{id}`), in double braces (`{{id}}`) or after
/// a colon (`:id`) match any single segment.
#[derive(Debug, Clone, PartialEq)]
pub struct MockRoute {
    pub method: String,
    pub path: String,
    pub reply: MockReply,
    pub segments: Vec<String>,
    /// How many segments are literal text. A route with more literal text is more specific, and wins.
    pub literal_count: usize,
}

impl MockRoute {
    pub fn new(method: &str, path: &str, reply: MockReply) -> Self {
        let segments = segments(path);
        let literal_count = segments.iter().filter(|s| !is_parameter(s)).count();
        Self {
            method: method.to_ascii_uppercase(),
            path: path.to_string(),
            reply,
            segments,
            literal_count,
        }
    }
}

/// Decides which route answers a request. Pure: no sockets, so it can be tested directly.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MockRouteTable {
    routes: Vec<MockRoute>,
}

impl MockRouteTable {
    pub fn new(routes: Vec<MockRoute>) -> Self {
        Self { routes }
    }

    pub fn len(&self) -> usize {
        self.routes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }

    /// The route that answers a request, or `None`. The query string and fragment are ignored.
    pub fn find(&self, method: &str, target: &str) -> Option<&MockRoute> {
        self.resolve(method, target).0
    }

    /// The reply for a request. Unknown paths get a 404, and known paths with another method get a 405 that
    /// lists the allowed methods. Both are JSON.
    pub fn respond(&self, method: &str, target: &str) -> MockReply {
        let (route, allowed) = self.resolve(method, target);
        if let Some(route) = route {
            return route.reply.clone();
        }

        let path = path_of(target);
        let upper = method.to_ascii_uppercase();
        if !allowed.is_empty() {
            let allow = allowed.iter().map(String::as_str).collect::<Vec<_>>().join(", ");
            return error_reply(
                405,
                &format!("{upper} is not allowed on {path}. Use {allow}."),
                vec![KeyValue::new("Allow", allow)],
            );
        }

        error_reply(404, &format!("No mock matches {upper} {path}."), Vec::new())
    }

    fn resolve(&self, method: &str, target: &str) -> (Option<&MockRoute>, BTreeSet<String>) {
        let request_segments = segments(&path_of(target));
        let upper = method.to_ascii_uppercase();
        let mut best: Option<&MockRoute> = None;
        let mut allowed = BTreeSet::new();

        for route in &self.routes {
            if !matches(&route.segments, &request_segments) {
                continue;
            }

            if route.method == upper {
                // Strictly more literal text wins, so the first registered route wins a tie.
                if best.is_none_or(|b| route.literal_count > b.literal_count) {
                    best = Some(route);
                }
            } else {
                allowed.insert(route.method.clone());
            }
        }

        (best, allowed)
    }
}

/// The path of a request target or URL. Scheme and host are dropped, and so are the query and fragment. A
/// leading variable such as `{{baseUrl}}` is dropped too, since the mock cannot know what it stands for.
pub fn path_of(target: &str) -> String {
    let mut text = match target.find(['?', '#']) {
        Some(cut) => &target[..cut],
        None => target,
    };

    if let Some(scheme_end) = text.find("://") {
        let after_authority = &text[scheme_end + 3..];
        text = match after_authority.find('/') {
            Some(slash) => &after_authority[slash..],
            None => "/",
        };
    } else if text.starts_with("{{") {
        text = match text.find("}}") {
            Some(close) => &text[close + 2..],
            None => "/",
        };
    }

    if text.starts_with('/') {
        text.to_string()
    } else {
        format!("/{text}")
    }
}

/// The non-empty segments of a path.
pub fn segments(path: &str) -> Vec<String> {
    path.split('/').filter(|s| !s.is_empty()).map(String::from).collect()
}

static PARAMETER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\{\{[^}]+\}\}|\{[^{}]+\}|:[A-Za-z_][\w-]*)$").expect("parameter pattern is valid")
});

/// Whether a template segment is a parameter, in any of the three styles.
pub fn is_parameter(segment: &str) -> bool {
    PARAMETER.is_match(segment)
}

/// Whether a template matches a request's segments: same count, and each literal segment equal.
pub fn matches(template: &[String], request: &[String]) -> bool {
    template.len() == request.len()
        && template
            .iter()
            .zip(request)
            .all(|(t, s)| is_parameter(t) || t == s)
}

fn error_reply(status_code: u16, message: &str, headers: Vec<KeyValue>) -> MockReply {
    let body = serde_json::to_vec(&json!({ "error": message, "source": "Ricochet mock server" })).unwrap_or_default();
    MockReply {
        status_code,
        headers,
        body,
        media_type: Some("application/json; charset=utf-8".to_string()),
    }
}

/// Turns saved examples into routes. Nothing here touches the network or the disk.
///
/// Examples with the same method and path as an earlier one are skipped, so the first wins.
pub fn routes_from_examples(examples: &[MockExample]) -> Vec<MockRoute> {
    let mut routes = Vec::new();
    let mut seen = HashSet::new();
    for example in examples {
        let path = path_of(&example.url);
        let method = example.method.to_ascii_uppercase();
        if !seen.insert(format!("{method} {path}")) {
            continue;
        }

        let reply = MockReply {
            status_code: example.status_code,
            headers: example.headers.clone(),
            body: example.body.clone().into_bytes(),
            media_type: example.media_type.clone(),
        };
        routes.push(MockRoute::new(&method, &path, reply));
    }

    routes
}

/// One route per operation in an OpenAPI 3 JSON description. Each answers with its first 2xx response: the
/// example in the description if there is one, otherwise a sample built from its schema. Paths are served
/// under the path of the first server, so `servers: [ { url: "https://api/v1" } ]` serves `/v1/pets`.
///
/// Errors with [`MockServerError`] when the text is not an OpenAPI 3 description in JSON.
pub fn routes_from_openapi(json: &str) -> Result<Vec<MockRoute>, MockServerError> {
    let root: Value = serde_json::from_str(json).map_err(|_| {
        MockServerError::new("This is not JSON. Ricochet reads OpenAPI 3 in JSON, and YAML is not supported yet.")
    })?;

    let is_openapi_3 = root
        .get("openapi")
        .and_then(Value::as_str)
        .is_some_and(|version| version.starts_with("3."));
    if !is_openapi_3 {
        return Err(MockServerError::new("This is not an OpenAPI 3 description."));
    }

    let paths = root
        .get("paths")
        .and_then(Value::as_object)
        .ok_or_else(|| MockServerError::new("The description has no paths, so there is nothing to mock."))?;

    let base_path = base_path(&root);
    let mut routes = Vec::new();
    let mut seen = HashSet::new();
    for (path_name, path_item) in paths {
        if !path_item.is_object() {
            continue;
        }

        for method in OPENAPI_METHODS {
            let Some(operation) = path_item.get(method).filter(|op| op.is_object()) else {
                continue;
            };

            let full_path = format!("{base_path}{path_name}");
            let upper = method.to_ascii_uppercase();
            if !seen.insert(format!("{upper} {full_path}")) {
                continue;
            }

            routes.push(MockRoute::new(&upper, &full_path, reply_for(operation, &root)));
        }
    }

    Ok(routes)
}

fn base_path(root: &Value) -> String {
    let url = root
        .get("servers")
        .and_then(Value::as_array)
        .and_then(|servers| servers.first())
        .and_then(|first| first.get("url"))
        .and_then(Value::as_str);

    match url {
        None => String::new(),
        Some(url) => {
            let path = path_of(url);
            let trimmed = path.trim_end_matches('/');
            if trimmed == "/" { String::new() } else { trimmed.to_string() }
        }
    }
}

fn reply_for(operation: &Value, root: &Value) -> MockReply {
    let Some(responses) = operation.get("responses").and_then(Value::as_object) else {
        return empty_reply(200);
    };

    // The lowest 2xx code first, then "2XX", then "default", which is answered as 200.
    let mut lowest: Option<(u16, &Value)> = None;
    for (key, response) in responses {
        let code = key.parse::<u16>().ok().filter(|_| key.len() == 3 && key.starts_with('2'));
        if let Some(code) = code
            && lowest.is_none_or(|(best, _)| code < best)
        {
            lowest = Some((code, response));
        }
    }

    let chosen = match lowest {
        Some(found) => found,
        None => match (responses.get("2XX"), responses.get("default")) {
            (Some(range), _) => (200, range),
            (None, Some(fallback)) => (200, fallback),
            (None, None) => return empty_reply(200),
        },
    };

    let response = deref(chosen.1, root, 0).unwrap_or(&Value::Null);
    let (media_type, body) = body_for(response, root);
    MockReply {
        status_code: chosen.0,
        headers: Vec::new(),
        body,
        media_type,
    }
}

fn empty_reply(status_code: u16) -> MockReply {
    MockReply {
        status_code,
        headers: Vec::new(),
        body: Vec::new(),
        media_type: None,
    }
}

fn body_for(response: &Value, root: &Value) -> (Option<String>, Vec<u8>) {
    let Some(content) = response.get("content").and_then(Value::as_object) else {
        return (None, Vec::new());
    };

    // application/json first, then any other JSON type such as application/problem+json.
    let pick = content
        .keys()
        .find(|name| name.eq_ignore_ascii_case("application/json"))
        .or_else(|| {
            content.keys().find(|name| {
                let lower = name.to_ascii_lowercase();
                lower.ends_with("/json") || lower.ends_with("+json")
            })
        });

    match pick {
        None => {
            // Not JSON: only a text example can be sent.
            let Some((first_type, first_media)) = content.iter().next() else {
                return (None, Vec::new());
            };
            let Some(media) = first_media.as_object() else {
                return (None, Vec::new());
            };
            let text = media.get("example").and_then(Value::as_str).unwrap_or("");
            (Some(first_type.clone()), text.as_bytes().to_vec())
        }
        Some(name) => {
            let media = &content[name];
            let value = if let Some(example) = media.get("example") {
                example.clone()
            } else if let Some(value) = first_example_value(media) {
                value
            } else if let Some(schema) = media.get("schema") {
                write_sample(schema, root, 0)
            } else {
                Value::Null
            };

            let body = serde_json::to_vec(&value).unwrap_or_default();
            (Some(name.clone()), body)
        }
    }
}

/// The `value` of the first entry in `examples`, if there is one.
fn first_example_value(media: &Value) -> Option<Value> {
    media
        .get("examples")
        .and_then(Value::as_object)
        .and_then(|examples| examples.values().next())
        .and_then(|first| first.get("value"))
        .cloned()
}

/// A sample value for a schema: its example, default, first enum value, merged allOf, first oneOf or anyOf,
/// or a value of its type. Local `$ref`s are followed. Anything it cannot follow becomes null.
fn write_sample(schema: &Value, root: &Value, depth: usize) -> Value {
    let Some(node) = deref(schema, root, 0) else {
        return Value::Null;
    };
    let Some(s) = node.as_object() else {
        return Value::Null;
    };
    if depth > MAX_SAMPLE_DEPTH {
        return Value::Null;
    }

    if let Some(example) = s.get("example") {
        return example.clone();
    }

    if let Some(default) = s.get("default") {
        return default.clone();
    }

    if let Some(first) = s.get("enum").and_then(Value::as_array).and_then(|values| values.first()) {
        return first.clone();
    }

    if let Some(parts) = s.get("allOf").and_then(Value::as_array) {
        let mut merged: BTreeMap<&str, &Value> = BTreeMap::new();
        for part in parts {
            let properties = deref(part, root, 0)
                .and_then(|p| p.get("properties"))
                .and_then(Value::as_object);
            if let Some(properties) = properties {
                for (name, property) in properties {
                    merged.insert(name.as_str(), property);
                }
            }
        }

        let mut out = Map::new();
        for (name, property) in merged {
            out.insert(name.to_string(), write_sample(property, root, depth + 1));
        }
        return Value::Object(out);
    }

    for choice in ["oneOf", "anyOf"] {
        if let Some(first) = s.get(choice).and_then(Value::as_array).and_then(|options| options.first()) {
            return write_sample(first, root, depth + 1);
        }
    }

    let kind = s.get("type").and_then(Value::as_str);
    if kind == Some("object") || (kind.is_none() && s.contains_key("properties")) {
        let mut out = Map::new();
        if let Some(properties) = s.get("properties").and_then(Value::as_object) {
            for (name, property) in properties {
                out.insert(name.clone(), write_sample(property, root, depth + 1));
            }
        }
        return Value::Object(out);
    }

    match kind {
        Some("array") => match s.get("items") {
            Some(items) => Value::Array(vec![write_sample(items, root, depth + 1)]),
            None => Value::Array(Vec::new()),
        },
        Some("string") => Value::String(string_sample(s)),
        Some("integer") | Some("number") => json!(0),
        Some("boolean") => Value::Bool(true),
        _ => Value::Null,
    }
}

fn string_sample(schema: &Map<String, Value>) -> String {
    match schema.get("format").and_then(Value::as_str) {
        Some("date-time") => "2026-01-01T00:00:00Z",
        Some("date") => "2026-01-01",
        Some("uuid") => "00000000-0000-0000-0000-000000000000",
        Some("email") => "user@example.com",
        Some("uri") | Some("url") => "https://example.com",
        _ => "string",
    }
    .to_string()
}

/// Follows local `#/…` references. Returns `None` when a reference cannot be followed.
fn deref<'a>(node: &'a Value, root: &'a Value, depth: usize) -> Option<&'a Value> {
    if depth > MAX_SAMPLE_DEPTH {
        return None;
    }
    if !node.is_object() {
        return Some(node);
    }
    let Some(reference) = node.get("$ref").and_then(Value::as_str) else {
        return Some(node);
    };
    if !reference.starts_with("#/") {
        return None;
    }

    // Value::pointer unescapes ~0 and ~1, as JSON Pointer requires.
    let target = root.pointer(&reference[1..])?;
    deref(target, root, depth + 1)
}

/// What the server needs while it runs: its address, the routes it answers from, the stop flag and the thread.
struct Running {
    address: SocketAddr,
    routes: Arc<RwLock<MockRouteTable>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

/// Runs a mock on 127.0.0.1 between `start` and `stop`. It stops when dropped.
///
/// The listener is a plain std socket, not an HTTP crate. A local mock needs only HTTP/1.1 with one request
/// per connection, and a socket the server owns can be closed on `stop` deterministically: `stop` returns
/// once the thread has dropped the listener, so the port is free straight away.
#[derive(Default)]
pub struct MockServer {
    running: Option<Running>,
}

impl MockServer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// The port in use while running, or `None`.
    pub fn port(&self) -> Option<u16> {
        self.running.as_ref().map(|r| r.address.port())
    }

    /// The address in use while running, or `None`. It is always a loopback address.
    pub fn address(&self) -> Option<SocketAddr> {
        self.running.as_ref().map(|r| r.address)
    }

    /// Starts the server on `port` for a port in the user range (`MIN_PORT` to `MAX_PORT`). `on_handled` runs
    /// on the server thread after each request is answered.
    pub fn start<F>(&mut self, port: u16, routes: MockRouteTable, on_handled: F) -> Result<(), MockServerError>
    where
        F: Fn(MockLogEntry) + Send + 'static,
    {
        if !(MIN_PORT..=MAX_PORT).contains(&port) {
            return Err(MockServerError::new(format!("Choose a port from {MIN_PORT} to {MAX_PORT}.")));
        }

        self.listen(port, routes, on_handled)
    }

    /// Starts the server on any port on 127.0.0.1, including 0, which asks the OS for a free port. The app
    /// calls `start`, which checks the user range first. This is for tests and for callers that check the
    /// port themselves.
    pub fn listen<F>(&mut self, port: u16, routes: MockRouteTable, on_handled: F) -> Result<(), MockServerError>
    where
        F: Fn(MockLogEntry) + Send + 'static,
    {
        if self.running.is_some() {
            return Err(MockServerError::new("The mock server is already running."));
        }

        let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|_| {
            MockServerError::new(format!(
                "Port {port} could not be used. Another program may already be using it."
            ))
        })?;
        let address = listener
            .local_addr()
            .map_err(|_| MockServerError::new("The mock server did not get an address."))?;

        let routes = Arc::new(RwLock::new(routes));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let routes = Arc::clone(&routes);
            let stop = Arc::clone(&stop);
            thread::spawn(move || serve(listener, &routes, &stop, &on_handled))
        };

        self.running = Some(Running {
            address,
            routes,
            stop,
            thread: Some(thread),
        });
        Ok(())
    }

    /// Swaps the routes for the requests that arrive from now on.
    pub fn update_routes(&self, routes: MockRouteTable) {
        if let Some(running) = &self.running {
            let mut current = running.routes.write().unwrap_or_else(|e| e.into_inner());
            *current = routes;
        }
    }

    /// Stops the server. It returns once the thread has closed the listener, so the port is free after this.
    /// A request already being answered finishes first, or is dropped after `IO_TIMEOUT`.
    pub fn stop(&mut self) {
        if let Some(mut running) = self.running.take() {
            running.stop.store(true, Ordering::SeqCst);
            if let Some(thread) = running.thread.take() {
                let _ = thread.join();
            }
        }
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Accepts connections until `stop` is set. The listener is owned here, so it closes when this returns.
fn serve<F: Fn(MockLogEntry)>(listener: TcpListener, routes: &RwLock<MockRouteTable>, stop: &AtomicBool, on_handled: &F) {
    if listener.set_nonblocking(true).is_err() {
        return;
    }

    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            // An accepted socket can inherit non-blocking mode, so it is switched back for its reads and writes.
            Ok((stream, _)) => {
                if stream.set_nonblocking(false).is_ok() {
                    answer(stream, routes, on_handled);
                }
            }
            // Nothing is waiting, or a connection failed before it was accepted. Look again shortly.
            Err(_) => thread::sleep(POLL_INTERVAL),
        }
    }
}

fn answer<F: Fn(MockLogEntry)>(mut stream: TcpStream, routes: &RwLock<MockRouteTable>, on_handled: &F) {
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));

    let Some(head) = read_request_head(&mut stream) else {
        let _ = stream.write_all(&to_wire(&empty_reply(400)));
        let _ = stream.shutdown(Shutdown::Both);
        return;
    };

    let reply = routes
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .respond(&head.method, &head.target);
    let status_code = reply.status_code;

    // A client that drops mid-reply must not stop the server, so a failed write is not logged and is ignored.
    if stream.write_all(&to_wire(&reply)).and_then(|()| stream.flush()).is_ok() {
        on_handled(MockLogEntry {
            at: Local::now(),
            method: head.method.to_ascii_uppercase(),
            path: path_of(&head.target),
            status_code,
        });
    }
    let _ = stream.shutdown(Shutdown::Both);
}

struct RequestHead {
    method: String,
    target: String,
}

/// Reads the request line and headers, then discards any body. Returns `None` for a malformed or unfinished
/// request, or one whose head is larger than `MAX_HEAD_BYTES`.
fn read_request_head(stream: &mut TcpStream) -> Option<RequestHead> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    let head_end = loop {
        if let Some(position) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break position + 4;
        }
        if buffer.len() > MAX_HEAD_BYTES {
            return None;
        }
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
    };

    let text = std::str::from_utf8(&buffer[..head_end]).ok()?;
    let mut lines = text.split("\r\n");
    let mut parts = lines.next()?.split(' ');
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    if method.is_empty() || target.is_empty() || !parts.next()?.starts_with("HTTP/") {
        return None;
    }

    let content_length: u64 = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse().ok())
        .unwrap_or(0);
    let already_read = (buffer.len() - head_end) as u64;
    let remaining = content_length.saturating_sub(already_read).min(MAX_DRAIN_BYTES);
    let mut body = Read::take(&mut *stream, remaining);
    let _ = io::copy(&mut body, &mut io::sink());

    Some(RequestHead { method, target })
}

/// The bytes of a reply: the status line, the headers, then the body. A header that would break the framing,
/// such as one with a line break in its value, is left out rather than sent.
fn to_wire(reply: &MockReply) -> Vec<u8> {
    let mut head = format!("HTTP/1.1 {} {}\r\n", reply.status_code, reason_phrase(reply.status_code));

    for header in &reply.headers {
        let owned_by_media_type = reply.media_type.is_some() && header.key.eq_ignore_ascii_case("Content-Type");
        let reserved = RESERVED_HEADERS.iter().any(|r| r.eq_ignore_ascii_case(&header.key));
        if owned_by_media_type || reserved || !is_header_name(&header.key) || has_line_break(&header.value) {
            continue;
        }
        head.push_str(&format!("{}: {}\r\n", header.key, header.value));
    }

    if let Some(media_type) = reply.media_type.as_deref().filter(|m| !has_line_break(m)) {
        head.push_str(&format!("Content-Type: {media_type}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", reply.body.len()));

    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(&reply.body);
    bytes
}

fn is_header_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_graphic() && b != b':')
}

fn has_line_break(value: &str) -> bool {
    value.contains(['\r', '\n', '\0'])
}

fn reason_phrase(status_code: u16) -> &'static str {
    match status_code {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "",
    }
}
