//! Port of `BuiltInFunctions.cs` (ADR 0008). Generated values written in the braces with parentheses,
//! such as `{{uuid.v4()}}`. The list is fixed, so a shared Collection cannot run code, read files or send a Secret.
//!
//! Known gap: `timestamp.format` supports the tokens `yyyy MM dd HH mm ss fff` and quoted literal text.
//! Other .NET format tokens are rejected, so the placeholder stays as written instead of being sent broken.

use std::sync::OnceLock;

use chrono::{SecondsFormat, Utc};
use rand::Rng;
use regex::Regex;

/// Postman's dynamic variable names, which act as these functions so imported Requests keep working.
const POSTMAN_ALIASES: &[(&str, &str)] = &[
    ("$guid", "uuid.v4()"),
    ("$timestamp", "timestamp.unix()"),
    ("$randomInt", "random.int(0, 1000)"),
];

/// The functions as they are written inside the braces, offered as suggestions while typing (ADR 0009).
pub const EXAMPLES: &[&str] = &[
    "uuid.v4()",
    "timestamp.iso8601()",
    "timestamp.unix()",
    "timestamp.unixMillis()",
    "timestamp.format('yyyy-MM-dd')",
    "random.int(1, 100)",
];

/// True for a Postman dynamic variable name that a built-in function replaces, such as `$guid`.
pub fn is_postman_alias(name: &str) -> bool {
    POSTMAN_ALIASES.iter().any(|(alias, _)| *alias == name.trim())
}

fn call_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?s)^([A-Za-z][A-Za-z0-9]*(?:\.[A-Za-z][A-Za-z0-9]*)+)\((.*)\)$").expect("valid pattern")
    })
}

fn whole_number() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"^-?[0-9]+$").expect("valid pattern"))
}

/// Evaluates a placeholder's name, such as `uuid.v4()`. Returns None when it is not a known function or its
/// arguments are wrong, so the placeholder stays as written and is reported as missing.
pub fn try_evaluate(name: &str) -> Option<String> {
    let mut text = name.trim().to_string();
    if let Some((_, target)) = POSTMAN_ALIASES.iter().find(|(alias, _)| *alias == text) {
        text = (*target).to_string();
    }

    let caps = call_pattern().captures(&text)?;
    let function = &caps[1];
    let args = parse_arguments(&caps[2])?;
    let now = Utc::now();

    match (function, args.len()) {
        ("uuid.v4", 0) => Some(uuid::Uuid::new_v4().to_string()),
        ("timestamp.iso8601", 0) => Some(now.to_rfc3339_opts(SecondsFormat::Millis, true)),
        ("timestamp.unix", 0) => Some(now.timestamp().to_string()),
        ("timestamp.unixMillis", 0) => Some(now.timestamp_millis().to_string()),
        ("timestamp.format", 1) if !args[0].is_empty() => format_timestamp(&now, &args[0]),
        ("random.int", 2) => {
            let low: i64 = args[0].parse().ok()?;
            let high: i64 = args[1].parse().ok()?;
            if low > high || high == i64::MAX {
                return None;
            }
            Some(rand::rng().random_range(low..=high).to_string())
        }
        _ => None,
    }
}

/// Formats a timestamp with the supported .NET-style tokens. Returns None for any token it does not support.
fn format_timestamp(now: &chrono::DateTime<Utc>, pattern: &str) -> Option<String> {
    let mut out = String::new();
    let mut rest = pattern;
    while let Some(first) = rest.chars().next() {
        if first == '\'' {
            let end = rest[1..].find('\'')?;
            out.push_str(&rest[1..1 + end]);
            rest = &rest[end + 2..];
            continue;
        }

        // A run of letters must be exactly one supported token, so "dddd" (a day name in .NET) is rejected, not read as "dd dd".
        let run = rest.chars().take_while(|c| *c == first).count();
        let token = ["yyyy", "MM", "dd", "HH", "mm", "ss", "fff"]
            .into_iter()
            .find(|t| t.len() == run && rest.starts_with(t));
        match token {
            Some(token) => {
                out.push_str(&now.format(match token {
                    "yyyy" => "%Y",
                    "MM" => "%m",
                    "dd" => "%d",
                    "HH" => "%H",
                    "mm" => "%M",
                    "ss" => "%S",
                    _ => "%3f",
                }).to_string());
                rest = &rest[token.len()..];
            }
            None if first.is_ascii_alphabetic() => return None,
            None => {
                out.push(first);
                rest = &rest[first.len_utf8()..];
            }
        }
    }
    Some(out)
}

/// Splits the arguments. Text goes in single quotes and numbers are written bare. A variable or another function
/// cannot be an argument, so anything else makes the whole call invalid.
fn parse_arguments(text: &str) -> Option<Vec<String>> {
    if text.trim().is_empty() {
        return Some(Vec::new());
    }

    let mut pieces = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for c in text.chars() {
        if c == '\'' {
            in_quotes = !in_quotes;
        }
        if c == ',' && !in_quotes {
            pieces.push(std::mem::take(&mut current));
        } else {
            current.push(c);
        }
    }

    if in_quotes {
        return None;
    }
    pieces.push(current);

    let mut args = Vec::new();
    for piece in pieces {
        let arg = piece.trim();
        if arg.len() >= 2 && arg.starts_with('\'') && arg.ends_with('\'') && !arg[1..arg.len() - 1].contains('\'') {
            args.push(arg[1..arg.len() - 1].to_string());
        } else if whole_number().is_match(arg) {
            args.push(arg.to_string());
        } else {
            return None;
        }
    }

    Some(args)
}
