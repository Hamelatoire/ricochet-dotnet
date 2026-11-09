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
const MAX_JSON_DEPTH: usize = 512;

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

/// Pretty-prints a JSON document with two-space indentation, keeping object key order and number text as written.
/// Returns `None` when the text is not valid JSON, so the caller shows it unchanged.
fn pretty_json(text: &str) -> Option<String> {
    let mut parser = JsonPretty { text, bytes: text.as_bytes(), pos: 0 };
    let mut out = String::new();
    parser.value(0, 0, &mut out)?;
    parser.skip_ws();
    (parser.pos == parser.bytes.len()).then_some(out)
}

struct JsonPretty<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl JsonPretty<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    /// Writes one value. `depth` counts enclosing containers; `level` is the indentation level of this value.
    fn value(&mut self, depth: usize, level: usize, out: &mut String) -> Option<()> {
        self.skip_ws();
        match self.peek()? {
            b'{' => self.object(depth, level, out),
            b'[' => self.array(depth, level, out),
            b'"' => {
                let s = self.string()?;
                write_string(out, &s);
                Some(())
            }
            b't' => self.literal("true", out),
            b'f' => self.literal("false", out),
            b'n' => self.literal("null", out),
            b'-' | b'0'..=b'9' => self.number(out),
            _ => None,
        }
    }

    fn object(&mut self, depth: usize, level: usize, out: &mut String) -> Option<()> {
        if depth >= MAX_JSON_DEPTH {
            return None;
        }
        self.pos += 1;
        self.skip_ws();
        if self.peek()? == b'}' {
            self.pos += 1;
            out.push_str("{}");
            return Some(());
        }

        out.push('{');
        loop {
            self.skip_ws();
            if self.peek()? != b'"' {
                return None;
            }
            newline_indent(out, level + 1);
            let key = self.string()?;
            write_string(out, &key);
            self.skip_ws();
            if self.peek()? != b':' {
                return None;
            }
            self.pos += 1;
            out.push_str(": ");
            self.value(depth + 1, level + 1, out)?;
            self.skip_ws();
            match self.peek()? {
                b',' => {
                    self.pos += 1;
                    out.push(',');
                }
                b'}' => {
                    self.pos += 1;
                    break;
                }
                _ => return None,
            }
        }

        newline_indent(out, level);
        out.push('}');
        Some(())
    }

    fn array(&mut self, depth: usize, level: usize, out: &mut String) -> Option<()> {
        if depth >= MAX_JSON_DEPTH {
            return None;
        }
        self.pos += 1;
        self.skip_ws();
        if self.peek()? == b']' {
            self.pos += 1;
            out.push_str("[]");
            return Some(());
        }

        out.push('[');
        loop {
            newline_indent(out, level + 1);
            self.value(depth + 1, level + 1, out)?;
            self.skip_ws();
            match self.peek()? {
                b',' => {
                    self.pos += 1;
                    out.push(',');
                }
                b']' => {
                    self.pos += 1;
                    break;
                }
                _ => return None,
            }
        }

        newline_indent(out, level);
        out.push(']');
        Some(())
    }

    /// Reads a string literal at the current quote and returns its decoded content.
    fn string(&mut self) -> Option<String> {
        self.pos += 1;
        let mut out = String::new();
        loop {
            let b = self.peek()?;
            match b {
                b'"' => {
                    self.pos += 1;
                    return Some(out);
                }
                b'\\' => {
                    let escape = *self.bytes.get(self.pos + 1)?;
                    self.pos += 2;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode_escape()?),
                        _ => return None,
                    }
                }
                0..=0x1F => return None,
                _ => {
                    let ch = self.text[self.pos..].chars().next()?;
                    out.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
    }

    /// Decodes the four hex digits after `\u`, joining a surrogate pair when there is one.
    fn unicode_escape(&mut self) -> Option<char> {
        let high = self.hex4()?;
        if (0xD800..0xDC00).contains(&high) {
            if self.peek()? != b'\\' || *self.bytes.get(self.pos + 1)? != b'u' {
                return None;
            }
            self.pos += 2;
            let low = self.hex4()?;
            if !(0xDC00..0xE000).contains(&low) {
                return None;
            }
            return char::from_u32(0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00));
        }
        char::from_u32(high)
    }

    fn hex4(&mut self) -> Option<u32> {
        let digits = self.text.get(self.pos..self.pos + 4)?;
        if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        self.pos += 4;
        u32::from_str_radix(digits, 16).ok()
    }

    fn literal(&mut self, word: &str, out: &mut String) -> Option<()> {
        if !self.text[self.pos..].starts_with(word) {
            return None;
        }
        self.pos += word.len();
        out.push_str(word);
        Some(())
    }

    fn number(&mut self, out: &mut String) -> Option<()> {
        let start = self.pos;
        if self.peek()? == b'-' {
            self.pos += 1;
        }
        match self.peek()? {
            b'0' => self.pos += 1,
            b'1'..=b'9' => {
                self.digits();
            }
            _ => return None,
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if !self.digits() {
                return None;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !self.digits() {
                return None;
            }
        }
        out.push_str(&self.text[start..self.pos]);
        Some(())
    }

    /// Skips ASCII digits, returning whether there was at least one.
    fn digits(&mut self) -> bool {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        self.pos > start
    }
}

fn newline_indent(out: &mut String, level: usize) {
    out.push('\n');
    for _ in 0..level {
        out.push_str("  ");
    }
}

/// Writes a JSON string literal, escaping only what JSON requires.
fn write_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}
