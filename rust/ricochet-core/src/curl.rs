//! Port of `CurlGenerator.cs` (ADR 0022). Shows a resolved Request as a curl command. The input must already have
//! its Variables applied, so the command is the one that would be sent. Secret values are masked unless the caller
//! asks to reveal them (ADR 0016).

use std::cmp::Reverse;

use crate::models::{ApiRequest, RequestAuth, auth_kinds};

/// What a masked Secret is replaced with.
pub const MASK: &str = "***";

/// Builds the curl command for `resolved`.
///
/// `secrets` are values known to be secret, such as Secret Variables. Auth values are always masked too.
/// `reveal` shows the real values instead.
pub fn generate(resolved: &ApiRequest, secrets: &[&str], reveal: bool) -> String {
    let masked = if reveal { Vec::new() } else { mask_list(resolved, secrets) };
    // A plain GET keeps the URL on the curl line. Any other method goes there instead, and the URL moves to the next line.
    let url = quote(&redact(&url_with_query_auth(resolved), &masked));
    let is_get = resolved.method.eq_ignore_ascii_case("GET");
    let first = if is_get {
        format!("curl {url}")
    } else {
        format!("curl -X {}", resolved.method.to_uppercase())
    };
    let mut parts: Vec<String> = Vec::new();
    if !is_get {
        parts.push(url);
    }

    let has_content_type = resolved
        .headers
        .iter()
        .any(|h| h.enabled && h.key.trim().eq_ignore_ascii_case("Content-Type"));
    if !resolved.body.is_empty() && !has_content_type {
        let content_type = format!("Content-Type: {}", guess_content_type(&resolved.body));
        parts.push(format!("-H {}", quote(&content_type)));
    }

    for header in resolved.headers.iter().filter(|h| h.enabled && !h.key.trim().is_empty()) {
        let line = format!("{}: {}", header.key.trim(), header.value);
        parts.push(format!("-H {}", quote(&redact(&line, &masked))));
    }

    if let Some(auth) = &resolved.auth {
        match auth.kind.as_str() {
            auth_kinds::BEARER if !auth.token.is_empty() => {
                let line = format!("Authorization: Bearer {}", auth.token);
                parts.push(format!("-H {}", quote(&redact(&line, &masked))));
            }
            auth_kinds::BASIC => {
                let credentials = format!("{}:{}", auth.username, auth.password);
                parts.push(format!("-u {}", quote(&redact(&credentials, &masked))));
            }
            auth_kinds::API_KEY if !auth.key_in_query && !auth.key_name.is_empty() => {
                let line = format!("{}: {}", auth.key_name, auth.key_value);
                parts.push(format!("-H {}", quote(&redact(&line, &masked))));
            }
            _ => {}
        }
    }

    if !resolved.body.is_empty() {
        parts.push(format!("--data-raw {}", quote(&redact(&resolved.body, &masked))));
    }

    if parts.is_empty() {
        first
    } else {
        let mut command = first;
        for part in parts {
            command.push_str(" \\\n  ");
            command.push_str(&part);
        }
        command
    }
}

/// The content type the sender would use for a body that has no Content-Type header of its own.
pub fn guess_content_type(body: &str) -> &'static str {
    match body.trim_start().chars().next() {
        Some('{' | '[') => "application/json",
        _ => "text/plain; charset=utf-8",
    }
}

/// Secrets that will be masked: the caller's list plus the auth values, longest first.
fn mask_list(resolved: &ApiRequest, secrets: &[&str]) -> Vec<String> {
    let auth = resolved.auth.as_ref();
    let candidates = secrets
        .iter()
        .copied()
        .chain([
            auth.map_or("", |a| a.token.as_str()),
            auth.map_or("", |a| a.password.as_str()),
            auth.map_or("", |a| a.key_value.as_str()),
        ])
        .filter(|value| !value.is_empty());

    let mut unique: Vec<&str> = Vec::new();
    for value in candidates {
        if !unique.contains(&value) {
            unique.push(value);
        }
    }
    // Stable, so values of the same length keep their order.
    unique.sort_by_key(|value| Reverse(value.encode_utf16().count()));
    unique.into_iter().map(str::to_string).collect()
}

fn redact(text: &str, secrets: &[String]) -> String {
    secrets
        .iter()
        .fold(text.to_string(), |current, secret| current.replace(secret.as_str(), MASK))
}

/// An API key sent in the query string is part of the URL, as the sender does.
fn url_with_query_auth(request: &ApiRequest) -> String {
    let url = request.url.trim();
    match &request.auth {
        Some(RequestAuth { kind, key_in_query: true, key_name, key_value, .. })
            if kind.as_str() == auth_kinds::API_KEY && !key_name.is_empty() =>
        {
            let pair = format!("{}={}", escape_data_string(key_name), escape_data_string(key_value));
            if url.contains('?') {
                format!("{url}&{pair}")
            } else {
                format!("{url}?{pair}")
            }
        }
        _ => url.to_string(),
    }
}

/// Matches .NET's `Uri.EscapeDataString`: unreserved characters stay, everything else is UTF-8 percent-encoded.
fn escape_data_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Single quotes, with embedded single quotes written as '\'' so the shell reads them back exactly.
fn quote(value: &str) -> String {
    let mut out = String::from("'");
    for c in value.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}
