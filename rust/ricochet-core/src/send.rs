//! Port of `src/Ricochet.Core/RequestSender.cs` (ADR 0091). Sends one Request over HTTP or HTTPS and returns the
//! response. It uses reqwest with rustls, so no OpenSSL or Schannel is needed, and it trusts the certificates in the
//! operating system's store, as the C# app does. Certificate checks are on by default (ADR 0079).
//!
//! Cookies are kept only within one run (`begin_run_cookies`, ADR 0012); single sends from the app keep none.
//! `send_cancellable` stops a send the person cancels. Not ported yet: automatic decompression, the Windows system proxy (reqwest
//! reads only the proxy environment variables), PKCS #12 client certificates (.pfx and .p12, which rustls cannot
//! read), resolving Secret references (ExternalSecrets) and the runner. The C# app stays the reference for those.

use std::borrow::Cow;
use std::fmt;
use std::io::{self, Read};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};

use reqwest::cookie::Jar;
use std::time::{Duration, Instant};

use base64::Engine;
use chrono::Utc;
use regex::Regex;
use reqwest::blocking::{Client, RequestBuilder, Response};
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue, WWW_AUTHENTICATE};
use reqwest::{Identity, Method, StatusCode, redirect};
use url::Url;

use crate::assertions::ResponseSnapshot;
use crate::auth_signing::{self, AuthError};
use crate::models::{ApiRequest, KeyValue, RequestAuth, auth_kinds};

/// The request timeout when the settings do not give one (ADR 0007).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
/// The most body bytes kept from one response (100 MiB), as in the C# app.
pub const DEFAULT_MAX_BODY_BYTES: u64 = 100 * 1024 * 1024;
/// Shown while certificate checks are off (ADR 0079).
pub const CERTIFICATE_CHECKS_OFF_WARNING: &str =
    "Certificate checks are off. Requests accept any server certificate, so the server's identity is not checked.";

static SCHEME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9+.\-]*://").expect("valid regex"));

/// A request that could not be sent, or failed. The message is for the user. Mirrors `RequestFailedException`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendError {
    message: String,
    cancelled: bool,
}

/// The message of a send the person cancelled.
pub const CANCELLED_MESSAGE: &str = "Cancelled";

impl SendError {
    fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), cancelled: false }
    }

    fn cancelled() -> Self {
        Self { message: CANCELLED_MESSAGE.to_string(), cancelled: true }
    }

    /// True when the send stopped because it was cancelled, not because it failed. The C# app shows "Cancelled"
    /// and keeps no History entry for it.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SendError {}

impl From<AuthError> for SendError {
    fn from(error: AuthError) -> Self {
        Self::new(error.message())
    }
}

/// The settings that shape every send. They apply to the requests sent after `RequestSender::configure`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SenderSettings {
    /// The limit for the whole request, from the start of connecting to the end of the body.
    pub timeout: Duration,
    pub follow_redirects: bool,
    /// Off accepts any certificate. Only turn it off on purpose (ADR 0079).
    pub verify_certificates: bool,
    /// Bytes kept from the body. A longer body is cut and marked truncated.
    pub max_body_bytes: u64,
    /// A proxy such as `proxy.example.com:8080`. Empty keeps the proxy from the environment (ADR 0026).
    pub proxy_url: String,
    /// A PEM file holding the client certificate and its unencrypted key. Empty sends no certificate.
    pub client_certificate_path: String,
}

impl Default for SenderSettings {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_TIMEOUT,
            follow_redirects: true,
            verify_certificates: true,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            proxy_url: String::new(),
            client_certificate_path: String::new(),
        }
    }
}

impl SenderSettings {
    /// The warning to show while certificate checks are off (ADR 0079), or None while they are on.
    pub fn certificate_warning(&self) -> Option<&'static str> {
        (!self.verify_certificates).then_some(CERTIFICATE_CHECKS_OFF_WARNING)
    }
}

/// What came back from one send. Mirrors `ApiResponse` in the C# app.
#[derive(Debug, Clone, PartialEq)]
pub struct ApiResponse {
    pub status_code: u16,
    /// The standard reason for the status, such as "OK". reqwest does not expose the server's own text.
    pub reason_phrase: String,
    pub headers: Vec<KeyValue>,
    /// The body bytes, cut at the size limit when `truncated` is set.
    pub body: Vec<u8>,
    /// The media type from Content-Type, without parameters, or None when there is no Content-Type.
    pub media_type: Option<String>,
    pub charset: Option<String>,
    /// From the start of the send to the end of the body.
    pub elapsed: Duration,
    pub truncated: bool,
    /// True when the send skipped certificate checks, so a pass does not prove who the server is.
    pub certificates_unverified: bool,
}

impl ApiResponse {
    /// The body as text. Bytes that are not UTF-8 become replacement characters.
    pub fn body_text(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.body)
    }

    /// The parts of the response that assertions and captures read (ADR 0006).
    pub fn snapshot(&self) -> ResponseSnapshot {
        ResponseSnapshot {
            status_code: i32::from(self.status_code),
            elapsed: self.elapsed,
            headers: self.headers.clone(),
            body: self.body_text().into_owned(),
        }
    }
}

/// Sends Requests with one set of settings. A send uses the settings in force when it starts.
pub struct RequestSender {
    settings: SenderSettings,
    /// The client for the current settings, or the problem that stops every send until they change.
    client: Result<Client, String>,
}

impl Default for RequestSender {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestSender {
    /// A sender with the default settings.
    pub fn new() -> Self {
        let settings = SenderSettings::default();
        let client = build_client(&settings);
        Self { settings, client }
    }

    pub fn settings(&self) -> &SenderSettings {
        &self.settings
    }

    /// Applies new settings to later sends. Returns the problem with the proxy or client certificate, to show beside
    /// those settings, or None when they can be used. While there is a problem, every send fails with it.
    pub fn configure(&mut self, settings: SenderSettings) -> Option<String> {
        self.client = build_client(&settings);
        self.settings = settings;
        self.client.as_ref().err().cloned()
    }

    /// Sends one Request and reads its response.
    ///
    /// # Errors
    /// Returns `SendError` when the Request is invalid (for example a control character, which is refused rather
    /// than removed, ADR 0075), when the settings cannot be used, when the connection fails, or when the timeout passes.
    pub fn send(&self, request: &ApiRequest) -> Result<ApiResponse, SendError> {
        self.send_cancellable(request, &AtomicBool::new(false))
    }

    /// `send`, stopped when `cancel` is set. The flag is checked before each request goes out and between chunks of
    /// the body. Waiting for the server's first answer cannot be interrupted, so a send cancelled then stops when that
    /// answer comes or the timeout passes, and the caller should stop waiting for it at once.
    ///
    /// # Errors
    /// As `send`, and a `SendError` whose `is_cancelled` is true when the flag was set.
    pub fn send_cancellable(&self, request: &ApiRequest, cancel: &AtomicBool) -> Result<ApiResponse, SendError> {
        let client = self.client.as_ref().map_err(|problem| SendError::new(problem.clone()))?;
        self.send_on(client, request, cancel)
    }

    /// Starts the cookies for one run (ADR 0012). They start empty, live only in the returned value, and are not
    /// shared with any other run. Dropping it drops them. Mirrors `BeginRunCookies`.
    pub fn begin_run_cookies(&self) -> RunCookies {
        let clients = self.client.as_ref().map_err(Clone::clone).and_then(|plain| {
            let jar = Arc::new(Jar::default());
            Ok((build_client_with(&self.settings, Some(jar))?, plain.clone()))
        });
        RunCookies { clients }
    }

    /// Sends a Request with the cookies of one run (ADR 0012). A Request that writes its own Cookie header is sent
    /// without the stored cookies, so they don't add to it.
    ///
    /// # Errors
    /// Same as `send`.
    pub fn send_in_run(&self, request: &ApiRequest, run: &RunCookies) -> Result<ApiResponse, SendError> {
        let (with_cookies, without_cookies) = run.clients.as_ref().map_err(|problem| SendError::new(problem.clone()))?;
        let own_cookie = request.headers.iter().any(|h| h.enabled && h.key.trim().eq_ignore_ascii_case("cookie"));
        self.send_on(if own_cookie { without_cookies } else { with_cookies }, request, &AtomicBool::new(false))
    }

    fn send_on(&self, client: &Client, request: &ApiRequest, cancel: &AtomicBool) -> Result<ApiResponse, SendError> {
        let timeout = self.settings.timeout;
        let started = Instant::now();

        let response = send_with_auth(client, request, timeout, cancel)?;
        let status = response.status();
        let headers = collect_headers(response.headers());
        let (media_type, charset) = content_type_parts(response.headers());
        let (body, truncated) = read_body(response, self.settings.max_body_bytes, timeout, cancel)?;

        Ok(ApiResponse {
            status_code: status.as_u16(),
            reason_phrase: status.canonical_reason().unwrap_or("").to_string(),
            headers,
            body,
            media_type,
            charset,
            elapsed: started.elapsed(),
            truncated,
            certificates_unverified: !self.settings.verify_certificates,
        })
    }
}

/// The cookies kept for one run (ADR 0012), in memory only. Dropping it drops them.
pub struct RunCookies {
    /// The client that keeps this run's cookies, and the sender's plain client for Requests with their own Cookie
    /// header. Or the problem that stops every send.
    clients: Result<(Client, Client), String>,
}

/// Builds the client for one set of settings. Proxy and certificate problems are returned as text for the user.
fn build_client(settings: &SenderSettings) -> Result<Client, String> {
    build_client_with(settings, None)
}

/// `build_client`, with a cookie jar when `cookies` is given. Without one, no cookies are kept.
fn build_client_with(settings: &SenderSettings, cookies: Option<Arc<Jar>>) -> Result<Client, String> {
    let mut builder = Client::builder();
    if let Some(jar) = cookies {
        builder = builder.cookie_provider(jar);
    }
    builder = builder
        .timeout(settings.timeout)
        .redirect(if settings.follow_redirects { redirect::Policy::default() } else { redirect::Policy::none() })
        .danger_accept_invalid_certs(!settings.verify_certificates);

    if let Some(proxy) = parse_proxy(&settings.proxy_url).map_err(|error| error.message().to_string())? {
        let proxy = reqwest::Proxy::all(proxy).map_err(|error| format!("The proxy could not be used: {error}"))?;
        builder = builder.proxy(proxy);
    }

    if let Some(identity) = load_client_certificate(&settings.client_certificate_path)? {
        builder = builder.identity(identity);
    }

    builder.build().map_err(|error| format!("The HTTP client could not be created: {error}"))
}

/// Reads a proxy address such as "proxy.example.com:8080". Empty means no explicit proxy. A user name or password
/// in the address is refused, because it would be kept in plain text, and proxy logins are not supported yet.
pub fn parse_proxy(text: &str) -> Result<Option<Url>, SendError> {
    let value = text.trim();
    if value.is_empty() {
        return Ok(None);
    }

    let candidate = if SCHEME.is_match(value) { value.to_string() } else { format!("http://{value}") };
    let uri = match Url::parse(&candidate) {
        Ok(uri) if matches!(uri.scheme(), "http" | "https") && uri.host_str().is_some() => uri,
        _ => {
            return Err(SendError::new(format!(
                "Invalid proxy address: {value}. Use a form such as http://proxy.example.com:8080."
            )));
        }
    };

    if !uri.username().is_empty() || uri.password().is_some() {
        return Err(SendError::new(
            "Proxy logins are not supported yet. Remove the user name and password from the proxy address.",
        ));
    }

    Ok(Some(uri))
}

/// Loads a client certificate from a PEM file. PKCS #12 files (.pfx, .p12) are refused with the reason, because
/// rustls cannot read them. The password of an encrypted key is not supported either.
fn load_client_certificate(path: &str) -> Result<Option<Identity>, String> {
    let path = path.trim();
    if path.is_empty() {
        return Ok(None);
    }

    if !Path::new(path).is_file() {
        return Err(format!("The client certificate file was not found: {path}"));
    }

    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".pfx") || lower.ends_with(".p12") {
        return Err(format!(
            "PKCS #12 client certificates (.pfx and .p12) are not supported yet. Convert {path} to one PEM file that holds the certificate and its unencrypted key."
        ));
    }

    let pem = std::fs::read(path).map_err(|error| format!("The client certificate could not be read from {path}: {error}"))?;
    Identity::from_pem(&pem)
        .map(Some)
        .map_err(|error| format!("The client certificate could not be loaded from {path}: {error}"))
}

/// Sends the request. Digest needs the server's challenge first, so a 401 with a Digest challenge is answered with one
/// more send of the same request (ADR 0024). The request is built again, so the body goes out in full both times.
fn send_with_auth(client: &Client, request: &ApiRequest, timeout: Duration, cancel: &AtomicBool) -> Result<Response, SendError> {
    check_cancel(cancel)?;
    let response = build_request(client, request, None)?.send().map_err(|error| transport_error(error, timeout))?;
    check_cancel(cancel)?;

    let Some(challenge) = digest_challenge(request, &response) else {
        return Ok(response);
    };
    drop(response);

    check_cancel(cancel)?;
    build_request(client, request, Some(&challenge))?.send().map_err(|error| transport_error(error, timeout))
}

/// The text after the "Digest" scheme of a 401's WWW-Authenticate header, or None when there is no such challenge.
fn digest_challenge(request: &ApiRequest, response: &Response) -> Option<String> {
    let digest = request.auth.as_ref().is_some_and(|auth| auth.kind == auth_kinds::DIGEST);
    if !digest || response.status() != StatusCode::UNAUTHORIZED {
        return None;
    }

    response
        .headers()
        .get_all(WWW_AUTHENTICATE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .map(str::trim_start)
        .find_map(|text| {
            let prefix = text.get(..7)?;
            prefix.eq_ignore_ascii_case("Digest ").then(|| text[7..].to_string())
        })
}

/// Builds the request: URL, headers, body, then auth. `challenge` is set only for the second Digest send.
fn build_request(client: &Client, request: &ApiRequest, challenge: Option<&str>) -> Result<RequestBuilder, SendError> {
    // Checked on the resolved values, so every value that reaches the wire passes the same check (ADR 0075).
    reject_control_characters("URL", &request.url)?;
    let method = Method::from_bytes(request.method.as_bytes())
        .map_err(|_| SendError::new(format!("Invalid method: {}.", request.method)))?;
    let url = apply_query_auth(parse_request_url(&request.url)?, request.auth.as_ref())?;

    let has_body = !request.body.is_empty();
    let mut headers = HeaderMap::new();
    for header in request.headers.iter().filter(|h| h.enabled && !h.key.trim().is_empty()) {
        let key = header.key.trim();
        reject_control_characters(&format!("header name \"{key}\""), key)?;
        reject_control_characters(&format!("value of \"{key}\""), &header.value)?;
        if !has_body && is_content_header(key) {
            continue; // A content header has nothing to describe without a body.
        }
        let invalid = || SendError::new(format!("Invalid header: \"{key}\"."));
        let name = HeaderName::from_bytes(key.as_bytes()).map_err(|_| invalid())?;
        let value = HeaderValue::from_str(&header.value).map_err(|_| invalid())?;
        headers.append(name, value);
    }

    if has_body && !headers.contains_key(CONTENT_TYPE) {
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(guess_content_type(&request.body)));
    }

    if let Some(auth) = request.auth.as_ref() {
        apply_auth(&mut headers, auth, request, &method, &url, challenge)?;
    }

    let mut builder = client.request(method, url).headers(headers);
    if has_body {
        builder = builder.body(request.body.clone());
    }
    Ok(builder)
}

/// Adds the auth headers for the request's kind. A header the user wrote by hand wins over the auth settings.
fn apply_auth(
    headers: &mut HeaderMap,
    auth: &RequestAuth,
    request: &ApiRequest,
    method: &Method,
    url: &Url,
    challenge: Option<&str>,
) -> Result<(), SendError> {
    let method = method.as_str();
    match auth.kind.as_str() {
        auth_kinds::BEARER => add_auth_header(headers, "Authorization", &format!("Bearer {}", auth.token))?,
        auth_kinds::BASIC => {
            let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", auth.username, auth.password));
            add_auth_header(headers, "Authorization", &format!("Basic {encoded}"))?;
        }
        auth_kinds::API_KEY if !auth.key_in_query && !auth.key_name.is_empty() => {
            add_auth_header(headers, &auth.key_name, &auth.key_value)?;
        }
        auth_kinds::DIGEST => {
            if let Some(challenge) = challenge {
                let value = auth_signing::digest_authorization(auth, method, &request_target(url), challenge, &client_nonce())?;
                add_auth_header(headers, "Authorization", &value)?;
            }
        }
        auth_kinds::OAUTH1 => {
            // Form fields are part of the signature, and a JSON body is not (RFC 5849 section 3.4.1.3).
            let form = if !request.body.is_empty() && is_form_body(headers) {
                auth_signing::parse_query(&request.body)
            } else {
                Vec::new()
            };
            let value = auth_signing::oauth1_authorization(auth, method, url, &form, &client_nonce(), Utc::now().timestamp())?;
            add_auth_header(headers, "Authorization", &value)?;
        }
        auth_kinds::AWS_SIG_V4 => {
            for header in auth_signing::aws_sig_v4_headers(auth, method, url, request.body.as_bytes(), Utc::now())? {
                add_auth_header(headers, &header.key, &header.value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn add_auth_header(headers: &mut HeaderMap, name: &str, value: &str) -> Result<(), SendError> {
    if name.is_empty() {
        return Ok(());
    }
    let invalid = || SendError::new(format!("Invalid auth header: \"{name}\"."));
    let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| invalid())?;
    if headers.contains_key(&header_name) {
        return Ok(());
    }
    reject_control_characters(&format!("value of \"{name}\""), value)?;
    let header_value = HeaderValue::from_str(value).map_err(|_| invalid())?;
    headers.insert(header_name, header_value);
    Ok(())
}

/// Adds the API key to the query string when the auth says so. The name and value are percent-encoded.
fn apply_query_auth(mut url: Url, auth: Option<&RequestAuth>) -> Result<Url, SendError> {
    let Some(auth) = auth else {
        return Ok(url);
    };
    if auth.kind != auth_kinds::API_KEY || !auth.key_in_query || auth.key_name.is_empty() {
        return Ok(url);
    }

    reject_control_characters(&format!("query parameter \"{}\"", auth.key_name), &auth.key_name)?;
    reject_control_characters(&format!("value of query parameter \"{}\"", auth.key_name), &auth.key_value)?;
    let pair = format!("{}={}", auth_signing::encode(&auth.key_name), auth_signing::encode(&auth.key_value));
    let query = match url.query() {
        Some(existing) if !existing.is_empty() => format!("{existing}&{pair}"),
        _ => pair,
    };
    url.set_query(Some(&query));
    Ok(url)
}

/// A line break can split a header or change what the server reads, so it is refused, never removed (ADR 0075).
/// A tab is allowed, because HTTP allows it in values.
fn reject_control_characters(what: &str, text: &str) -> Result<(), SendError> {
    match text.chars().find(|&c| c != '\t' && c.is_control()) {
        Some(c) => Err(SendError::new(format!(
            "The {what} contains a control character (U+{:04X}), which is not allowed. Remove it.",
            c as u32
        ))),
        None => Ok(()),
    }
}

/// Reads the request URL. A URL without a scheme is taken as http. Only http and https are allowed.
fn parse_request_url(raw: &str) -> Result<Url, SendError> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(SendError::new("Enter a URL."));
    }

    let candidate = if SCHEME.is_match(value) { value.to_string() } else { format!("http://{value}") };
    match Url::parse(&candidate) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => Ok(url),
        _ => Err(SendError::new(format!("Invalid URL: {value}"))),
    }
}

/// The path and query as they go on the request line, which Digest signs.
fn request_target(url: &Url) -> String {
    match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_string(),
    }
}

fn client_nonce() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn is_content_header(key: &str) -> bool {
    key.get(..8).is_some_and(|prefix| prefix.eq_ignore_ascii_case("content-"))
}

fn is_form_body(headers: &HeaderMap) -> bool {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|media| media.trim().eq_ignore_ascii_case("application/x-www-form-urlencoded"))
}

/// The Content-Type for a body that has none, from its first character.
fn guess_content_type(body: &str) -> &'static str {
    if body.trim_start().starts_with(['{', '[']) {
        "application/json"
    } else {
        "text/plain; charset=utf-8"
    }
}

fn read_body(mut response: Response, max_bytes: u64, timeout: Duration, cancel: &AtomicBool) -> Result<(Vec<u8>, bool), SendError> {
    let mut body = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    // Read in chunks, so a cancel stops a long download between them.
    while (body.len() as u64) < max_bytes {
        check_cancel(cancel)?;
        let room = usize::try_from(max_bytes - body.len() as u64).unwrap_or(usize::MAX).min(chunk.len());
        match response.read(&mut chunk[..room]) {
            Ok(0) => break,
            Ok(read) => body.extend_from_slice(&chunk[..read]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(read_error(error, timeout)),
        }
    }

    // Truncated only when a byte follows the limit, so a body of exactly the limit is complete.
    let truncated = body.len() as u64 >= max_bytes && has_more(&mut response, timeout)?;
    Ok((body, truncated))
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), SendError> {
    if cancel.load(Ordering::SeqCst) { Err(SendError::cancelled()) } else { Ok(()) }
}

fn has_more(response: &mut Response, timeout: Duration) -> Result<bool, SendError> {
    let mut probe = [0u8; 1];
    loop {
        match response.read(&mut probe) {
            Ok(read) => return Ok(read > 0),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(read_error(error, timeout)),
        }
    }
}

fn read_error(error: io::Error, timeout: Duration) -> SendError {
    let timed_out = error.kind() == io::ErrorKind::TimedOut
        || error
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<reqwest::Error>())
            .is_some_and(reqwest::Error::is_timeout);
    if timed_out {
        timeout_error(timeout)
    } else {
        SendError::new(error.to_string())
    }
}

fn transport_error(error: reqwest::Error, timeout: Duration) -> SendError {
    if error.is_timeout() {
        timeout_error(timeout)
    } else {
        SendError::new(error.to_string())
    }
}

fn timeout_error(timeout: Duration) -> SendError {
    SendError::new(format!("Timed out after {:.0} seconds.", timeout.as_secs_f64()))
}

fn collect_headers(map: &HeaderMap) -> Vec<KeyValue> {
    map.iter()
        .map(|(name, value)| KeyValue::new(name.as_str(), String::from_utf8_lossy(value.as_bytes()).into_owned()))
        .collect()
}

/// The media type and charset from Content-Type, such as "application/json" and "utf-8".
fn content_type_parts(headers: &HeaderMap) -> (Option<String>, Option<String>) {
    let Some(value) = headers.get(CONTENT_TYPE).and_then(|value| value.to_str().ok()) else {
        return (None, None);
    };

    let mut parts = value.split(';');
    let media = parts.next().map(str::trim).filter(|media| !media.is_empty()).map(str::to_string);
    let charset = parts.find_map(|part| {
        let (name, value) = part.split_once('=')?;
        name.trim().eq_ignore_ascii_case("charset").then(|| value.trim().trim_matches('"').to_string())
    });
    (media, charset)
}
