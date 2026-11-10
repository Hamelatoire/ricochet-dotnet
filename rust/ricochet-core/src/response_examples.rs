//! Port of `src/Ricochet.Core/ResponseExamples.cs` (ADR 0023). Turns the last response into a saved example, with
//! credentials masked before the example is stored. Writing the example into the Request file is the store's job and
//! is not ported here.
//!
//! Masking matches the C# app. Sensitive headers are hidden outright, as they are in History. Secret values are
//! replaced wherever they were echoed back: Secret Variables, captured values, the auth secrets of the Request as
//! sent, and sensitive headers that were sent. Values shorter than four characters are left alone, so that unrelated
//! output is not scrambled.
//!
//! `secrets_to_mask` ports `CollectionRunner.SecretsToMask`. The runner is not ported yet, and will reuse it then.
//! `is_literal` and `is_sensitive_header` are copies of `SecretRules` (`WorkspaceSecrets.cs`) until that is ported.

use std::cmp::Reverse;
use std::fmt;
use std::sync::OnceLock;

use regex::Regex;

use crate::models::{ApiRequest, KeyValue, ResponseExample};
use crate::response_compare::HIDDEN_VALUE;
use crate::response_format;
use crate::send::ApiResponse;

/// Shorter values are not masked, because masking them would scramble unrelated output.
const MIN_MASKED_LENGTH: usize = 4;

// Same names as `SecretRules.SensitiveHeaders`, matched without regard to case.
const SENSITIVE_HEADERS: [&str; 8] = [
    "Authorization",
    "Proxy-Authorization",
    "Cookie",
    "Set-Cookie",
    "X-Api-Key",
    "Api-Key",
    "X-Auth-Token",
    "X-Access-Token",
];

/// Why a response cannot be saved as an example. The messages are the ones the C# app shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExampleError {
    /// The body was cut at the size limit, so it would not be the whole response.
    TooLarge,
    /// The body is binary, which an example cannot hold.
    Binary,
}

impl ExampleError {
    pub fn message(self) -> &'static str {
        match self {
            Self::TooLarge => "The response is too large to save in full, so it was not saved as an example.",
            Self::Binary => "Binary responses cannot be saved as examples.",
        }
    }
}

impl fmt::Display for ExampleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for ExampleError {}

/// Builds the example for a response. `sent` is the Request as it was sent (resolved), so its literal credentials
/// are known and can be found wherever they were echoed back. `variables` and `captured` supply the other secrets.
pub fn create(
    name: &str,
    sent: &ApiRequest,
    variables: &[KeyValue],
    captured: &[KeyValue],
    response: &ApiResponse,
) -> Result<ResponseExample, ExampleError> {
    if response.truncated {
        return Err(ExampleError::TooLarge);
    }

    let formatted = response_format::format(response);
    if formatted.is_binary {
        return Err(ExampleError::Binary);
    }

    // Sensitive headers that were sent are masked wherever they are echoed, as well as the values the runner masks.
    let header_secrets = sent
        .headers
        .iter()
        .filter(|h| h.enabled && is_sensitive_header(&h.key) && is_literal(&h.value))
        .map(|h| h.value.clone());
    let secrets = distinct_longest_first(
        secrets_to_mask(variables, &[], captured, sent)
            .into_iter()
            .chain(header_secrets.filter(|v| utf16_len(v) >= MIN_MASKED_LENGTH)),
    );

    Ok(ResponseExample {
        name: name.trim().to_string(),
        status_code: i32::from(response.status_code),
        reason_phrase: response.reason_phrase.clone(),
        headers: response.headers.iter().map(|h| mask_header(h, &secrets)).collect(),
        media_type: response.media_type.clone(),
        body: mask(&formatted.text, &secrets),
        ..ResponseExample::default()
    })
}

/// The secret values for one send, longest first. Port of `CollectionRunner.SecretsToMask`. Captured values are all
/// masked, because nothing marks which ones are tokens. Overrides are the values given on the command line.
pub fn secrets_to_mask(
    variables: &[KeyValue],
    overrides: &[KeyValue],
    captured: &[KeyValue],
    resolved: &ApiRequest,
) -> Vec<String> {
    let mut values: Vec<String> = variables
        .iter()
        .filter(|v| v.is_secret)
        .chain(overrides)
        .chain(captured)
        .map(|v| v.value.clone())
        .collect();
    if let Some(auth) = &resolved.auth {
        values.extend(auth_secret_values(auth));
    }

    distinct_longest_first(values.into_iter())
}

/// The auth fields that hold secrets. Port of `WorkspaceSecrets.AuthSecretValues`.
fn auth_secret_values(auth: &crate::models::RequestAuth) -> Vec<String> {
    let empty = String::new();
    let details = auth.details.as_ref();
    [
        &auth.token,
        &auth.password,
        &auth.key_value,
        details.map_or(&empty, |d| &d.consumer_secret),
        details.map_or(&empty, |d| &d.token_secret),
        details.map_or(&empty, |d| &d.aws_secret_access_key),
        details.map_or(&empty, |d| &d.aws_session_token),
    ]
    .into_iter()
    .cloned()
    .collect()
}

/// Drops values too short to mask and repeats, then sorts the rest longest first. Equal lengths keep their order,
/// as `OrderByDescending` does. Longest first means a value that contains another is masked whole.
fn distinct_longest_first(values: impl Iterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for value in values {
        if utf16_len(&value) >= MIN_MASKED_LENGTH && !out.contains(&value) {
            out.push(value);
        }
    }
    out.sort_by_key(|value| Reverse(utf16_len(value)));
    out
}

/// Sensitive headers are hidden outright, as they are in History. Other values have any secret masked.
fn mask_header(header: &KeyValue, secrets: &[String]) -> KeyValue {
    let value = if is_sensitive_header(&header.key) && is_literal(&header.value) {
        HIDDEN_VALUE.to_string()
    } else {
        mask(&header.value, secrets)
    };
    KeyValue { value, ..header.clone() }
}

fn mask(text: &str, secrets: &[String]) -> String {
    secrets
        .iter()
        .fold(text.to_string(), |current, secret| current.replace(secret.as_str(), HIDDEN_VALUE))
}

/// True when the text holds an actual value, not just `{{placeholders}}` that are safe to share.
fn is_literal(value: &str) -> bool {
    static PLACEHOLDER: OnceLock<Regex> = OnceLock::new();
    let placeholder = PLACEHOLDER.get_or_init(|| Regex::new(r"\{\{[^{}]+\}\}").expect("valid pattern"));
    !placeholder.replace_all(value, "").trim().is_empty()
}

fn is_sensitive_header(name: &str) -> bool {
    let name = name.trim();
    SENSITIVE_HEADERS.iter().any(|header| header.eq_ignore_ascii_case(name))
}

/// Length in UTF-16 code units, as `string.Length` counts in the C# app.
fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}
