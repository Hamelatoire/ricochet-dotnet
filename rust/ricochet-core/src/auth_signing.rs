//! Port of `src/Ricochet.Core/AuthSigning.cs` (ADR 0024). Builds the Authorization headers for Digest,
//! OAuth 1.0 and AWS Signature Version 4. These are pure functions: the nonce, timestamp and clock are
//! passed in, so the results can be checked against known examples. Sending is not ported here.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use base64::Engine;
use chrono::{DateTime, Utc};
use hmac::{Hmac, KeyInit, Mac};
use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256};
use url::Url;

use crate::models::{AuthDetails, KeyValue, RequestAuth};

/// A signing failure the user can act on, such as a missing field or an unsupported challenge.
/// Mirrors `RequestFailedException` in the C# app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthError {
    message: String,
}

impl AuthError {
    fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AuthError {}

/// Percent-encodes as RFC 3986 requires, which OAuth 1.0 and AWS both use: only unreserved characters stay as they are.
pub fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for &b in value.as_bytes() {
        if b < 128 && (b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Splits a query string into decoded pairs. A pair without "=" has an empty value.
/// A leading "?" is ignored, and "+" decodes to a space.
pub fn parse_query(query: &str) -> Vec<(String, String)> {
    url::form_urlencoded::parse(query.trim_start_matches('?').as_bytes())
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect()
}

// Digest (RFC 7616, checked against the MD5 example of RFC 2617)

/// The Authorization value answering a Digest challenge. `challenge` is the text after the "Digest" scheme
/// in WWW-Authenticate. Supports MD5 and SHA-256 with qop "auth" or no qop.
pub fn digest_authorization(
    auth: &RequestAuth,
    method: &str,
    request_target: &str,
    challenge: &str,
    client_nonce: &str,
) -> Result<String, AuthError> {
    let parameters = parse_digest_challenge(challenge);
    let (Some(realm), Some(nonce)) = (parameters.get("realm"), parameters.get("nonce")) else {
        return Err(AuthError::new("The server's Digest challenge has no realm or nonce."));
    };

    let algorithm = parameters.get("algorithm").map(String::as_str).unwrap_or("MD5");
    if !matches!(algorithm, "MD5" | "SHA-256") {
        return Err(AuthError::new(format!(
            "The server asks for Digest algorithm {algorithm}, which is not supported. Use MD5 or SHA-256."
        )));
    }

    let qop = match parameters.get("qop") {
        Some(offered) => {
            if !offered.split(',').any(|o| o.trim() == "auth") {
                return Err(AuthError::new(format!(
                    "The server asks for Digest qop {offered}, which is not supported. Use auth."
                )));
            }
            Some("auth")
        }
        None => None,
    };

    const NC: &str = "00000001";
    let ha1 = hash(algorithm, &format!("{}:{realm}:{}", auth.username, auth.password));
    let ha2 = hash(algorithm, &format!("{method}:{request_target}"));
    let response = match qop {
        None => hash(algorithm, &format!("{ha1}:{nonce}:{ha2}")),
        Some(qop) => hash(algorithm, &format!("{ha1}:{nonce}:{NC}:{client_nonce}:{qop}:{ha2}")),
    };

    let mut header = format!(
        "Digest username=\"{}\", realm=\"{}\", nonce=\"{}\", uri=\"{}\", algorithm={algorithm}",
        quote(&auth.username),
        quote(realm),
        quote(nonce),
        quote(request_target),
    );
    if qop.is_some() {
        header.push_str(&format!(", qop=auth, nc={NC}, cnonce=\"{}\"", quote(client_nonce)));
    }
    header.push_str(&format!(", response=\"{response}\""));
    if let Some(opaque) = parameters.get("opaque") {
        header.push_str(&format!(", opaque=\"{}\"", quote(opaque)));
    }

    Ok(header)
}

/// Reads name=value pairs from a challenge, where values may be quoted and contain commas.
/// Names are lowercased, because challenge parameter names are case-insensitive. A name without a value is skipped.
pub fn parse_digest_challenge(parameters: &str) -> HashMap<String, String> {
    let chars: Vec<char> = parameters.chars().collect();
    let n = chars.len();
    let mut result = HashMap::new();
    let mut i = 0;
    while i < n {
        while i < n && matches!(chars[i], ',' | ' ' | '\t') {
            i += 1;
        }

        let start = i;
        while i < n && !matches!(chars[i], '=' | ',') {
            i += 1;
        }

        let key: String = chars[start..i].iter().collect::<String>().trim().to_string();
        if i >= n || chars[i] != '=' {
            continue;
        }

        i += 1; // the "="
        let value = if i < n && chars[i] == '"' {
            i += 1;
            let mut sb = String::new();
            while i < n && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < n {
                    i += 1;
                }

                sb.push(chars[i]);
                i += 1;
            }

            i += 1; // the closing quote
            sb
        } else {
            let value_start = i;
            while i < n && chars[i] != ',' {
                i += 1;
            }

            chars[value_start..i].iter().collect::<String>().trim().to_string()
        };

        if !key.is_empty() {
            result.insert(key.to_ascii_lowercase(), value);
        }
    }

    result
}

fn hash(algorithm: &str, text: &str) -> String {
    let bytes = text.as_bytes();
    if algorithm == "SHA-256" { hex(&Sha256::digest(bytes)) } else { hex(&Md5::digest(bytes)) }
}

fn quote(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

// OAuth 1.0 (RFC 5849, HMAC-SHA1)

/// The Authorization header for an OAuth 1.0 request signed with HMAC-SHA1. `body_parameters` are the decoded
/// form fields of a form-encoded body, which take part in the signature. Query parameters are read from `uri`.
pub fn oauth1_authorization(
    auth: &RequestAuth,
    method: &str,
    uri: &Url,
    body_parameters: &[(String, String)],
    nonce: &str,
    timestamp: i64,
) -> Result<String, AuthError> {
    let default = AuthDetails::default();
    let details = auth.details.as_ref().unwrap_or(&default);
    if details.consumer_key.is_empty() {
        return Err(AuthError::new("Enter the OAuth 1.0 consumer key."));
    }

    let mut oauth: Vec<(String, String)> = vec![
        ("oauth_consumer_key".into(), details.consumer_key.clone()),
        ("oauth_nonce".into(), nonce.into()),
        ("oauth_signature_method".into(), "HMAC-SHA1".into()),
        ("oauth_timestamp".into(), timestamp.to_string()),
    ];
    if !auth.token.is_empty() {
        oauth.push(("oauth_token".into(), auth.token.clone()));
    }
    oauth.push(("oauth_version".into(), "1.0".into()));

    let mut everything: Vec<(String, String)> = parse_query(uri.query().unwrap_or(""))
        .into_iter()
        .chain(body_parameters.iter().cloned())
        .chain(oauth.iter().cloned())
        .map(|(key, value)| (encode(&key), encode(&value)))
        .collect();
    everything.sort();
    let normalized = everything.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");

    let mut base_uri = uri.clone();
    base_uri.set_query(None);
    base_uri.set_fragment(None);
    let base_string = format!("{}&{}&{}", method.to_uppercase(), encode(base_uri.as_str()), encode(&normalized));

    let signing_key = format!("{}&{}", encode(&details.consumer_secret), encode(&details.token_secret));
    let signature = base64::engine::general_purpose::STANDARD.encode(hmac_sha1(signing_key.as_bytes(), base_string.as_bytes()));

    oauth.push(("oauth_signature".into(), signature));
    let fields = oauth.iter().map(|(k, v)| format!("{k}=\"{}\"", encode(v))).collect::<Vec<_>>().join(", ");
    Ok(format!("OAuth {fields}"))
}

// AWS Signature Version 4

/// The headers that sign a request for AWS Signature Version 4: X-Amz-Date, X-Amz-Security-Token when there is a
/// session token, and Authorization. The signed headers are host, x-amz-date and, when there is one, x-amz-security-token.
pub fn aws_sig_v4_headers(
    auth: &RequestAuth,
    method: &str,
    uri: &Url,
    body: &[u8],
    now: DateTime<Utc>,
) -> Result<Vec<KeyValue>, AuthError> {
    let default = AuthDetails::default();
    let details = auth.details.as_ref().unwrap_or(&default);
    if details.aws_access_key_id.is_empty() || details.aws_secret_access_key.is_empty() {
        return Err(AuthError::new("Enter the AWS access key id and secret access key."));
    }
    if details.aws_region.is_empty() || details.aws_service.is_empty() {
        return Err(AuthError::new("Enter the AWS region and service."));
    }

    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = &amz_date[..8];
    let host_name = uri.host_str().unwrap_or("");
    let host = match uri.port() {
        Some(port) => format!("{host_name}:{port}"),
        None => host_name.to_string(),
    };

    // A BTreeMap keeps the header names in ordinal order, as the signature requires.
    let mut headers: BTreeMap<&str, String> = BTreeMap::new();
    headers.insert("host", host);
    headers.insert("x-amz-date", amz_date.clone());
    if !details.aws_session_token.is_empty() {
        headers.insert("x-amz-security-token", details.aws_session_token.clone());
    }

    let canonical_headers: String = headers.iter().map(|(k, v)| format!("{k}:{}\n", v.trim())).collect();
    let signed_headers = headers.keys().copied().collect::<Vec<_>>().join(";");

    let canonical_uri = {
        let joined = uri
            .path()
            .split('/')
            .map(|segment| encode(&unescape_data_string(segment)))
            .collect::<Vec<_>>()
            .join("/");
        if joined.is_empty() { "/".to_string() } else { joined }
    };

    let mut query: Vec<(String, String)> = parse_query(uri.query().unwrap_or(""))
        .into_iter()
        .map(|(key, value)| (encode(&key), encode(&value)))
        .collect();
    query.sort();
    let canonical_query = query.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");

    let canonical_request = format!(
        "{}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{}",
        method.to_uppercase(),
        hex(&Sha256::digest(body)),
    );
    let scope = format!("{date}/{}/{}/aws4_request", details.aws_region, details.aws_service);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        hex(&Sha256::digest(canonical_request.as_bytes())),
    );

    let k_date = hmac_sha256(format!("AWS4{}", details.aws_secret_access_key).as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, details.aws_region.as_bytes());
    let k_service = hmac_sha256(&k_region, details.aws_service.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"aws4_request");
    let signature = hex(&hmac_sha256(&k_signing, string_to_sign.as_bytes()));

    let mut result = vec![KeyValue::new("X-Amz-Date", amz_date)];
    if !details.aws_session_token.is_empty() {
        result.push(KeyValue::new("X-Amz-Security-Token", details.aws_session_token.clone()));
    }
    result.push(KeyValue::new(
        "Authorization",
        format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
            details.aws_access_key_id,
        ),
    ));
    Ok(result)
}

fn hmac_sha1(key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(key).expect("HMAC accepts keys of any length");
    mac.update(message);
    mac.finalize().into_bytes().to_vec()
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC accepts keys of any length");
    mac.update(message);
    mac.finalize().into_bytes().to_vec()
}

/// Decodes %XX escapes and leaves anything else as it is, like `Uri.UnescapeDataString` (no "+" handling).
fn unescape_data_string(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escape = if bytes[i] == b'%' && i + 2 < bytes.len() {
            hex_digit(bytes[i + 1]).zip(hex_digit(bytes[i + 2]))
        } else {
            None
        };
        match escape {
            Some((hi, lo)) => {
                out.push(hi * 16 + lo);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_digit(b: u8) -> Option<u8> {
    (b as char).to_digit(16).map(|d| d as u8)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
