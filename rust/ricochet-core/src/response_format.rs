//! Port of `src/Ricochet.Core/ResponseFormatter.cs` (ADR 0091). Turns a response body into the text the response
//! panel shows: JSON is pretty-printed, a body that holds NUL bytes is replaced by a short note, and anything else is
//! shown as text. Assertions, response comparison and saved examples read the same text.
//!
//! Charsets: the C# version calls `Encoding.GetEncoding`, which knows only the encodings .NET has built in. Those are
//! ported here: UTF-8, UTF-16 (little and big endian), US-ASCII and ISO-8859-1. Any other name, including UTF-32 and
//! windows-1252, is treated as unknown and decoded as UTF-8, which is what the C# app does for names it cannot resolve.
//! UTF-32 is therefore not decoded as UTF-32 yet.
//!
//! `pretty_json` lives here, not in `response_compare`, so that both modules share one printer.

use crate::send::ApiResponse;

/// The most UTF-16 units that are pretty-printed. Longer text is shown as written, as in the C# app.
pub const MAX_PRETTY_PRINT_CHARS: usize = 32 * 1024 * 1024;
/// How many leading bytes are checked for NUL bytes when deciding whether a body is binary.
const BINARY_SNIFF_BYTES: usize = 8192;
/// Deepest nesting of arrays and objects that is pretty-printed. Deeper JSON is shown as written.
const MAX_JSON_DEPTH: usize = 512;

/// The text shown for a response body. `is_json` is set when the text was pretty-printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormattedBody {
    pub text: String,
    pub is_json: bool,
    pub is_binary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Charset {
    Utf8,
    Utf16Le,
    Utf16Be,
    Ascii,
    Latin1,
}

/// Formats a response body for display, pretty-printing JSON. Mirrors `ResponseFormatter.Format`.
pub fn format(response: &ApiResponse) -> FormattedBody {
    let charset = resolve_charset(response.charset.as_deref());
    if charset.is_none() && looks_binary(&response.body) {
        return FormattedBody {
            text: format!("[Binary response, {} bytes]", group_thousands(response.body.len())),
            is_json: false,
            is_binary: true,
        };
    }

    let decoded = decode(&response.body, charset.unwrap_or(Charset::Utf8));
    let text = decoded.trim_start_matches('\u{feff}');
    if response.truncated
        || utf16_len(text) > MAX_PRETTY_PRINT_CHARS
        || !might_be_json(response.media_type.as_deref(), text)
    {
        return plain(text);
    }

    match pretty_json(text) {
        Some(pretty) => FormattedBody { text: pretty, is_json: true, is_binary: false },
        None => plain(text),
    }
}

fn plain(text: &str) -> FormattedBody {
    FormattedBody { text: text.to_string(), is_json: false, is_binary: false }
}

/// Maps a charset name to one of the encodings ported here, matching names without regard to case. None when the
/// name is missing, blank or not one the C# app can resolve.
fn resolve_charset(charset: Option<&str>) -> Option<Charset> {
    let name = charset?.trim().trim_matches('"');
    match name.to_ascii_lowercase().as_str() {
        "utf-8" | "utf8" => Some(Charset::Utf8),
        "utf-16" | "utf-16le" | "unicode" => Some(Charset::Utf16Le),
        "utf-16be" | "unicodefffe" => Some(Charset::Utf16Be),
        "us-ascii" | "ascii" => Some(Charset::Ascii),
        "iso-8859-1" => Some(Charset::Latin1),
        _ => None,
    }
}

fn decode(body: &[u8], charset: Charset) -> String {
    match charset {
        Charset::Utf8 => String::from_utf8_lossy(body).into_owned(),
        Charset::Utf16Le => decode_utf16(body, false),
        Charset::Utf16Be => decode_utf16(body, true),
        // Bytes above 127 are not ASCII, so they become U+FFFD, as .NET's ASCII decoder does.
        Charset::Ascii => body.iter().map(|&b| if b < 0x80 { char::from(b) } else { char::REPLACEMENT_CHARACTER }).collect(),
        Charset::Latin1 => body.iter().map(|&b| char::from(b)).collect(),
    }
}

fn decode_utf16(body: &[u8], big_endian: bool) -> String {
    let units = body.chunks_exact(2).map(|pair| {
        let bytes = [pair[0], pair[1]];
        if big_endian { u16::from_be_bytes(bytes) } else { u16::from_le_bytes(bytes) }
    });
    let mut text: String = char::decode_utf16(units)
        .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect();
    // A lone final byte is not a whole code unit, so it is shown as a replacement, as .NET does.
    if body.len() % 2 == 1 {
        text.push(char::REPLACEMENT_CHARACTER);
    }
    text
}

fn looks_binary(body: &[u8]) -> bool {
    body[..body.len().min(BINARY_SNIFF_BYTES)].contains(&0)
}

/// Mirrors `ResponseFormatter.MightBeJson`: a JSON media type, or text that starts with `{` or `[` after whitespace.
fn might_be_json(media_type: Option<&str>, text: &str) -> bool {
    if media_type.is_some_and(|m| m.to_ascii_lowercase().contains("json")) {
        return true;
    }

    text.trim_start().starts_with(['{', '['])
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Writes a count with a thousands separator, as `N0` does in the C# app's culture.
fn group_thousands(count: usize) -> String {
    let digits = count.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Pretty-prints a JSON document with two-space indentation, keeping object key order and number text as written.
/// Returns `None` when the text is not valid JSON, so the caller shows it unchanged.
pub fn pretty_json(text: &str) -> Option<String> {
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
