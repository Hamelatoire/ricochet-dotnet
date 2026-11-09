//! Compares two responses for the side-by-side view (ADR 0070). Port of `src/Ricochet.Core/ResponseComparison.cs`,
//! together with the parts of `ResponseFormatter.cs` that it relies on.
//!
//! Stand-in input: [`ResponseInput`] is the minimal view of a response this module needs. It will be replaced by
//! the real response type once that is ported, and then this module should take that type instead. HTTP sending
//! is not ported here.
//!
//! Known differences from the C# version, until the response type and the secrets rules are ported:
//! - Bodies are decoded as UTF-8 only. The C# version also honours a `charset` on the response.
//! - Responses are never marked truncated, so the "do not pretty-print truncated bodies" rule does not apply.
//! - The sensitive-header list and the `[hidden]` marker are defined here, and must be kept in step with
//!   `SecretRules` (`WorkspaceSecrets.cs`) until that module is ported.

use std::cmp::Reverse;

use crate::models::KeyValue;
use crate::response_format::pretty_json;

/// Minimal response input for comparison. See the module docs for why this is a stand-in.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResponseInput {
    pub status: u16,
    pub headers: Vec<KeyValue>,
    /// Raw body bytes. Kept as bytes rather than text so that binary bodies can be detected, as in C#.
    pub body: Vec<u8>,
    pub media_type: Option<String>,
}

/// How a row of a side-by-side body comparison differs. Same rows match on both sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    Same,
    Removed,
    Added,
    Changed,
}

/// One row of a side-by-side body comparison. A side without the line has no line number either.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideBySideLine {
    pub kind: DiffKind,
    pub previous_number: Option<usize>,
    pub previous: Option<String>,
    pub current_number: Option<usize>,
    pub current: Option<String>,
}

/// A header on either response. A hidden header (a sensitive one) shows only that it is hidden, and is never
/// counted as changed, so the comparison cannot reveal its value (ADR 0070).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderDifference {
    pub name: String,
    pub previous: Option<String>,
    pub current: Option<String>,
    pub hidden: bool,
}

impl HeaderDifference {
    pub fn changed(&self) -> bool {
        !self.hidden && self.previous != self.current
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseComparison {
    /// Rows of the formatted bodies, empty when the body was not compared.
    pub body: Vec<SideBySideLine>,
    /// Why the body was not compared, or `None` when it was.
    pub body_note: Option<String>,
    /// Every header name on either response, in order.
    pub headers: Vec<HeaderDifference>,
}

/// Marker that stands in for a secret value or a hidden header value. Same text as `SecretRules.HiddenValue`.
pub const HIDDEN_VALUE: &str = "[hidden]";

// Masking very short values would scramble unrelated output, as in ResponseExamples.
const MIN_MASKED_LENGTH: usize = 4;
// The line-by-line table is a grid of line pairs, so very large bodies are not compared.
const MAX_LINE_PAIRS: u64 = 4_000_000;
// Bodies longer than this are shown as written rather than pretty-printed.
const MAX_PRETTY_PRINT_BYTES: usize = 32 * 1024 * 1024;
const BINARY_SNIFF_BYTES: usize = 8192;

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

/// Compares two responses. Bodies are compared after pretty-printing JSON, so whitespace alone does not make every
/// line differ. Secret values are masked before comparing, so the output never shows them.
pub fn compare(previous: &ResponseInput, current: &ResponseInput, secrets: &[&str]) -> ResponseComparison {
    let masks = masks_for(secrets);
    let (body, body_note) = compare_bodies(previous, current, &masks);
    ResponseComparison {
        body,
        body_note,
        headers: compare_headers(&previous.headers, &current.headers, &masks),
    }
}

fn masks_for<'a>(secrets: &[&'a str]) -> Vec<&'a str> {
    let mut seen = std::collections::HashSet::new();
    let mut masks: Vec<&str> = secrets
        .iter()
        .copied()
        .filter(|s| utf16_len(s) >= MIN_MASKED_LENGTH && seen.insert(*s))
        .collect();
    // Stable sort, so equal-length secrets keep their order, as OrderByDescending does.
    masks.sort_by_key(|s| Reverse(utf16_len(s)));
    masks
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

fn mask(text: &str, masks: &[&str]) -> String {
    masks
        .iter()
        .fold(text.to_string(), |current, secret| current.replace(*secret, HIDDEN_VALUE))
}

fn compare_bodies(
    previous: &ResponseInput,
    current: &ResponseInput,
    masks: &[&str],
) -> (Vec<SideBySideLine>, Option<String>) {
    let (Some(before), Some(after)) = (format_body(previous), format_body(current)) else {
        return (Vec::new(), Some("Binary responses are not compared.".to_string()));
    };

    let a = split_lines(&mask(&before, masks));
    let b = split_lines(&mask(&after, masks));
    if a.len() as u64 * b.len() as u64 > MAX_LINE_PAIRS {
        return (Vec::new(), Some("The bodies are too large to compare line by line.".to_string()));
    }

    (diff_lines(&a, &b), None)
}

fn split_lines(text: &str) -> Vec<String> {
    text.replace("\r\n", "\n").split('\n').map(str::to_string).collect()
}

/// Formats a body for comparison, as `ResponseFormatter.Format` does. `None` means the body is binary.
fn format_body(response: &ResponseInput) -> Option<String> {
    let sniff = &response.body[..response.body.len().min(BINARY_SNIFF_BYTES)];
    if sniff.contains(&0) {
        return None;
    }

    let decoded = String::from_utf8_lossy(&response.body);
    let text = decoded.trim_start_matches('\u{feff}');
    if text.len() > MAX_PRETTY_PRINT_BYTES || !might_be_json(response.media_type.as_deref(), text) {
        return Some(text.to_string());
    }

    Some(pretty_json(text).unwrap_or_else(|| text.to_string()))
}

fn might_be_json(media_type: Option<&str>, text: &str) -> bool {
    if media_type.is_some_and(|m| m.to_ascii_lowercase().contains("json")) {
        return true;
    }

    matches!(text.trim_start().as_bytes().first(), Some(b'{' | b'['))
}

/// Longest common subsequence of lines, then a walk that pairs each run of changes into Changed rows.
fn diff_lines(a: &[String], b: &[String]) -> Vec<SideBySideLine> {
    let (n, m) = (a.len(), b.len());
    let cols = m + 1;
    let mut lcs = vec![0u32; (n + 1) * cols];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i * cols + j] = if a[i] == b[j] {
                lcs[(i + 1) * cols + j + 1] + 1
            } else {
                lcs[(i + 1) * cols + j].max(lcs[i * cols + j + 1])
            };
        }
    }

    let mut rows = Vec::new();
    let mut removed: Vec<usize> = Vec::new();
    let mut added: Vec<usize> = Vec::new();
    let (mut x, mut y) = (0, 0);
    while x < n || y < m {
        if x < n && y < m && a[x] == b[y] {
            flush_changes(&mut rows, &mut removed, &mut added, a, b);
            rows.push(SideBySideLine {
                kind: DiffKind::Same,
                previous_number: Some(x + 1),
                previous: Some(a[x].clone()),
                current_number: Some(y + 1),
                current: Some(b[y].clone()),
            });
            x += 1;
            y += 1;
        } else if y < m && (x >= n || lcs[x * cols + y + 1] >= lcs[(x + 1) * cols + y]) {
            added.push(y);
            y += 1;
        } else {
            removed.push(x);
            x += 1;
        }
    }

    flush_changes(&mut rows, &mut removed, &mut added, a, b);
    rows
}

fn flush_changes(
    rows: &mut Vec<SideBySideLine>,
    removed: &mut Vec<usize>,
    added: &mut Vec<usize>,
    a: &[String],
    b: &[String],
) {
    let count = removed.len().max(added.len());
    for k in 0..count {
        let left = removed.get(k).copied();
        let right = added.get(k).copied();
        let kind = match (left, right) {
            (Some(_), Some(_)) => DiffKind::Changed,
            (Some(_), None) => DiffKind::Removed,
            _ => DiffKind::Added,
        };
        rows.push(SideBySideLine {
            kind,
            previous_number: left.map(|i| i + 1),
            previous: left.map(|i| a[i].clone()),
            current_number: right.map(|j| j + 1),
            current: right.map(|j| b[j].clone()),
        });
    }

    removed.clear();
    added.clear();
}

fn is_sensitive_header(name: &str) -> bool {
    let trimmed = name.trim();
    SENSITIVE_HEADERS.iter().any(|h| h.eq_ignore_ascii_case(trimmed))
}

fn compare_headers(previous: &[KeyValue], current: &[KeyValue], masks: &[&str]) -> Vec<HeaderDifference> {
    let before = joined(previous);
    let after = joined(current);

    let mut names: Vec<&str> = before.iter().map(|(name, _)| name.as_str()).collect();
    for (name, _) in &after {
        if !before.iter().any(|(b, _)| b.eq_ignore_ascii_case(name)) {
            names.push(name);
        }
    }

    names
        .into_iter()
        .map(|name| {
            let hidden = is_sensitive_header(name);
            let old = lookup(&before, name);
            let new = lookup(&after, name);
            if hidden {
                return HeaderDifference {
                    name: name.to_string(),
                    previous: old.map(|_| HIDDEN_VALUE.to_string()),
                    current: new.map(|_| HIDDEN_VALUE.to_string()),
                    hidden: true,
                };
            }

            HeaderDifference {
                name: name.to_string(),
                previous: old.map(|v| mask(v, masks)),
                current: new.map(|v| mask(v, masks)),
                hidden: false,
            }
        })
        .collect()
}

fn lookup<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// Header values by name, ignoring case, in first-seen order. Repeated headers are joined with commas, as HTTP allows.
fn joined(headers: &[KeyValue]) -> Vec<(String, String)> {
    let mut values: Vec<(String, String)> = Vec::new();
    for header in headers {
        let position = values.iter().position(|(n, _)| n.eq_ignore_ascii_case(&header.key));
        match position {
            Some(i) => {
                values[i].1.push_str(", ");
                values[i].1.push_str(&header.value);
            }
            None => values.push((header.key.clone(), header.value.clone())),
        }
    }
    values
}
