//! Splits one line of pretty-printed JSON into coloured spans for the response body. It only classifies what is on
//! the line: text that is not JSON comes back as plain spans, so nothing is ever dropped or reordered.

/// What a span of a JSON line is, which decides its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    /// An object key, quotes included.
    Key,
    /// A string value, quotes included.
    String,
    Number,
    /// `true`, `false` or `null`.
    Literal,
    /// Brackets, braces, commas, colons and anything unrecognised.
    Plain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub kind: SpanKind,
    pub text: String,
}

/// One line as an indent width and its spans. The indent is the count of leading spaces, drawn as padding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColoredLine {
    pub indent: usize,
    pub spans: Vec<Span>,
}

/// Colours one line. Joining the spans' text gives the line back without its leading spaces.
pub fn color_line(line: &str) -> ColoredLine {
    let trimmed = line.trim_start_matches(' ');
    let indent = line.len() - trimmed.len();
    let chars: Vec<char> = trimmed.chars().collect();
    let mut spans: Vec<Span> = Vec::new();
    let mut plain = String::new();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        let (kind, end) = if c == '"' {
            let end = string_end(&chars, i);
            let is_key = chars[end..].iter().find(|c| !c.is_whitespace()) == Some(&':');
            (if is_key { SpanKind::Key } else { SpanKind::String }, end)
        } else if c == '-' || c.is_ascii_digit() {
            let end = run_end(&chars, i, |c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E'));
            (SpanKind::Number, end)
        } else if c.is_ascii_alphabetic() {
            let end = run_end(&chars, i, |c| c.is_ascii_alphabetic());
            let word: String = chars[i..end].iter().collect();
            let kind = if matches!(word.as_str(), "true" | "false" | "null") { SpanKind::Literal } else { SpanKind::Plain };
            (kind, end)
        } else {
            (SpanKind::Plain, i + 1)
        };

        let text: String = chars[i..end].iter().collect();
        if kind == SpanKind::Plain {
            plain.push_str(&text);
        } else {
            if !plain.is_empty() {
                spans.push(Span { kind: SpanKind::Plain, text: std::mem::take(&mut plain) });
            }
            spans.push(Span { kind, text });
        }
        i = end;
    }
    if !plain.is_empty() {
        spans.push(Span { kind: SpanKind::Plain, text: plain });
    }
    ColoredLine { indent, spans }
}

/// The index just past the closing quote of the string that opens at `start`, or the end of the line.
fn string_end(chars: &[char], start: usize) -> usize {
    let mut i = start + 1;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 2,
            '"' => return i + 1,
            _ => i += 1,
        }
    }
    chars.len()
}

fn run_end(chars: &[char], start: usize, keep: impl Fn(char) -> bool) -> usize {
    let mut i = start;
    while i < chars.len() && keep(chars[i]) {
        i += 1;
    }
    i
}
