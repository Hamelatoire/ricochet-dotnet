//! Port of `Assertions.cs` (ADR 0006): the checks a Request makes on a response, and the captures that save a
//! value from it for later Requests. Only the evaluation is here. The response type, its body formatting and
//! sending are not ported yet, so the checks read a `ResponseSnapshot` that the caller builds.
//!
//! Source and comparison names are kept as strings, as in the C# file, so an unknown name in a file is reported
//! as a failed check instead of failing to load.

use std::cell::OnceCell;
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::KeyValue;

/// Where a check reads from. Names are written to files, so they must not change.
pub mod sources {
    pub const STATUS: &str = "status";
    pub const TIME: &str = "time";
    pub const HEADER: &str = "header";
    pub const BODY: &str = "body";
    pub const JSON: &str = "json";
}

/// How a check compares what it read with the expected text. Names are written to files.
pub mod ops {
    pub const EQUAL: &str = "equals";
    pub const NOT_EQUAL: &str = "notEquals";
    pub const CONTAINS: &str = "contains";
    pub const NOT_CONTAINS: &str = "notContains";
    pub const EXISTS: &str = "exists";
    pub const NOT_EXISTS: &str = "notExists";
    pub const LESS_THAN: &str = "lessThan";
    pub const GREATER_THAN: &str = "greaterThan";
    pub const MATCHES: &str = "matches";
}

/// Longest text shown in a failure message, in characters. Longer text is cut and followed by an ellipsis.
const MAX_SHOWN_LENGTH: usize = 200;

fn yes() -> bool {
    true
}

/// One expected condition on a response (ADR 0006).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RequestAssertion {
    #[serde(default = "yes")]
    pub enabled: bool,
    pub source: String,
    pub path: String,
    pub op: String,
    pub expected: String,
}

impl Default for RequestAssertion {
    fn default() -> Self {
        Self {
            enabled: true,
            source: sources::STATUS.to_string(),
            path: String::new(),
            op: ops::EQUAL.to_string(),
            expected: String::new(),
        }
    }
}

/// The result of one check. `message` is empty when it passed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssertionResult {
    pub description: String,
    pub passed: bool,
    pub message: String,
}

/// Saves a value from a response into a Variable, for later Requests to use (ADR 0006).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RequestCapture {
    #[serde(default = "yes")]
    pub enabled: bool,
    pub source: String,
    pub path: String,
    pub variable: String,
}

impl Default for RequestCapture {
    fn default() -> Self {
        Self {
            enabled: true,
            source: sources::JSON.to_string(),
            path: String::new(),
            variable: String::new(),
        }
    }
}

/// The values captured from one response, and the captures that failed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CaptureOutcome {
    pub values: Vec<KeyValue>,
    pub failures: Vec<AssertionResult>,
}

/// The parts of a response the checks read. Built by the caller, because the response type is not ported yet.
///
/// `body` is the text the body and JSON checks see. In the C# app that is the formatted body, which the
/// response formatter produces, so the caller must pass the same text.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResponseSnapshot {
    pub status_code: i32,
    pub elapsed: Duration,
    pub headers: Vec<KeyValue>,
    pub body: String,
}

/// Runs every enabled assertion, in order. Disabled ones produce no result.
pub fn evaluate(assertions: &[RequestAssertion], response: &ResponseSnapshot) -> Vec<AssertionResult> {
    let json = OnceCell::new();
    assertions
        .iter()
        .filter(|a| a.enabled)
        .map(|assertion| {
            let description = describe(assertion);
            match read(&assertion.source, &assertion.path, response, &json) {
                Err(problem) => AssertionResult { description, passed: false, message: problem },
                Ok(actual) => compare(assertion, description, actual.as_deref()),
            }
        })
        .collect()
}

/// Runs every enabled capture. A capture that finds nothing is a failure, because later Requests depend on it.
pub fn evaluate_captures(captures: &[RequestCapture], response: &ResponseSnapshot) -> CaptureOutcome {
    let json = OnceCell::new();
    let mut outcome = CaptureOutcome::default();

    for capture in captures.iter().filter(|c| c.enabled) {
        let variable = capture.variable.trim();
        let name = if variable.is_empty() { "(unnamed)" } else { variable };
        let description = format!("capture {name} from {} {}", capture.source, capture.path).trim_end().to_string();

        if variable.is_empty() {
            outcome.failures.push(AssertionResult {
                description,
                passed: false,
                message: "it has no variable name".to_string(),
            });
            continue;
        }

        match read(&capture.source, &capture.path, response, &json) {
            Err(problem) => outcome.failures.push(AssertionResult { description, passed: false, message: problem }),
            Ok(None) => outcome.failures.push(AssertionResult {
                description,
                passed: false,
                message: "not found".to_string(),
            }),
            Ok(Some(value)) if value == "null" => outcome.failures.push(AssertionResult {
                description,
                passed: false,
                message: "not found".to_string(),
            }),
            Ok(Some(value)) => outcome.values.push(KeyValue::new(variable, value)),
        }
    }

    outcome
}

/// A short sentence for a check, such as `status equals 200`, shown in results.
pub fn describe(assertion: &RequestAssertion) -> String {
    let subject = match assertion.source.as_str() {
        sources::STATUS => "status".to_string(),
        sources::TIME => "time in ms".to_string(),
        sources::BODY => "body".to_string(),
        _ => format!("{} {}", assertion.source, assertion.path),
    };

    let op = match assertion.op.as_str() {
        ops::EQUAL => "equals",
        ops::NOT_EQUAL => "does not equal",
        ops::CONTAINS => "contains",
        ops::NOT_CONTAINS => "does not contain",
        ops::EXISTS => "exists",
        ops::NOT_EXISTS => "does not exist",
        ops::LESS_THAN => "is less than",
        ops::GREATER_THAN => "is greater than",
        ops::MATCHES => "matches",
        other => other,
    };

    if assertion.op == ops::EXISTS || assertion.op == ops::NOT_EXISTS {
        format!("{subject} {op}")
    } else {
        format!("{subject} {op} {}", assertion.expected)
    }
}

/// Finds a value in a response. `Ok(None)` means not found. `Err` means the check can't look at all.
/// A JSON null is found, and comes back as the text `null`.
fn read(
    source: &str,
    path: &str,
    response: &ResponseSnapshot,
    json: &OnceCell<Option<Value>>,
) -> Result<Option<String>, String> {
    match source {
        sources::STATUS => Ok(Some(response.status_code.to_string())),
        sources::TIME => Ok(Some(response.elapsed.as_millis().to_string())),
        sources::HEADER => {
            let wanted = path.trim();
            Ok(response
                .headers
                .iter()
                .find(|h| h.key.eq_ignore_ascii_case(wanted))
                .map(|h| h.value.clone()))
        }
        sources::BODY => Ok(Some(response.body.clone())),
        sources::JSON => {
            let root = json
                .get_or_init(|| serde_json::from_str::<Value>(&response.body).ok())
                .as_ref()
                .ok_or_else(|| "the response is not valid JSON".to_string())?;
            match JsonPath::find(root, path)? {
                None => Ok(None),
                Some(Value::String(text)) => Ok(Some(text)),
                Some(other) => Ok(Some(other.to_string())),
            }
        }
        other => Err(format!("unknown source \"{other}\"")),
    }
}

fn compare(assertion: &RequestAssertion, description: String, actual: Option<&str>) -> AssertionResult {
    let expected = assertion.expected.as_str();
    let result = |passed: bool| AssertionResult {
        description: description.clone(),
        passed,
        message: if passed {
            String::new()
        } else {
            match actual {
                None => "not found".to_string(),
                Some(text) => format!("got {}", shorten(text)),
            }
        },
    };

    if assertion.op == ops::EXISTS {
        return result(actual.is_some());
    }
    if assertion.op == ops::NOT_EXISTS {
        return AssertionResult {
            description,
            passed: actual.is_none(),
            message: match actual {
                None => String::new(),
                Some(text) => format!("found {}", shorten(text)),
            },
        };
    }
    let Some(actual) = actual else {
        return result(false);
    };

    match assertion.op.as_str() {
        ops::EQUAL => result(are_equal(actual, expected)),
        ops::NOT_EQUAL => result(!are_equal(actual, expected)),
        ops::CONTAINS => result(actual.contains(expected)),
        ops::NOT_CONTAINS => result(!actual.contains(expected)),
        ops::LESS_THAN | ops::GREATER_THAN => {
            let Some(left) = try_number(actual) else {
                return AssertionResult {
                    description,
                    passed: false,
                    message: format!("got {}, which is not a number", shorten(actual)),
                };
            };
            let Some(right) = try_number(expected) else {
                return AssertionResult {
                    description,
                    passed: false,
                    message: format!("expected \"{expected}\" is not a number"),
                };
            };
            result(if assertion.op == ops::LESS_THAN { left < right } else { left > right })
        }
        ops::MATCHES => match Regex::new(expected) {
            Ok(pattern) => result(pattern.is_match(actual)),
            Err(error) => AssertionResult {
                description,
                passed: false,
                message: format!("invalid pattern: {error}"),
            },
        },
        other => AssertionResult {
            description,
            passed: false,
            message: format!("unknown comparison \"{other}\""),
        },
    }
}

/// 1.0 and 1 are the same value in JSON, even though the text differs.
fn are_equal(actual: &str, expected: &str) -> bool {
    actual == expected
        || matches!((try_number(actual), try_number(expected)), (Some(a), Some(b)) if a == b)
}

fn try_number(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok().filter(|value| value.is_finite())
}

fn shorten(text: &str) -> String {
    if text.chars().count() <= MAX_SHOWN_LENGTH {
        text.to_string()
    } else {
        let head: String = text.chars().take(MAX_SHOWN_LENGTH).collect();
        format!("{head}…")
    }
}

/// Finds a value by a path such as `items[0].name` or `$.a['b.c']`. A trailing `.length` counts an array.
pub struct JsonPath;

impl JsonPath {
    /// `Ok(None)` when the path isn't there. `Err` is returned only when the path itself is malformed.
    pub fn find(root: &Value, path: &str) -> Result<Option<Value>, String> {
        let trimmed = path.trim();
        let text = trimmed.strip_prefix('$').unwrap_or(trimmed);
        let bytes = text.as_bytes();
        let mut value = root.clone();
        let mut i = 0;

        while i < bytes.len() {
            match bytes[i] {
                b'.' => i += 1,
                b'[' => {
                    let Some(offset) = text[i..].find(']') else {
                        return Err(format!("invalid path \"{path}\": missing ]"));
                    };
                    let close = i + offset;
                    let inner = text[i + 1..close].trim();
                    i = close + 1;

                    let quoted = inner.len() >= 2
                        && matches!(inner.as_bytes()[0], b'\'' | b'"')
                        && inner.as_bytes()[inner.len() - 1] == inner.as_bytes()[0];

                    if quoted {
                        match step(&value, &inner[1..inner.len() - 1]) {
                            Some(next) => value = next,
                            None => return Ok(None),
                        }
                    } else if !inner.is_empty() && inner.bytes().all(|b| b.is_ascii_digit()) {
                        let Ok(index) = inner.parse::<usize>() else {
                            return Err(format!("invalid path \"{path}\": [{inner}] should be a number or a quoted name"));
                        };
                        match value.as_array().and_then(|items| items.get(index)) {
                            Some(next) => value = next.clone(),
                            None => return Ok(None),
                        }
                    } else {
                        return Err(format!("invalid path \"{path}\": [{inner}] should be a number or a quoted name"));
                    }
                }
                _ => {
                    let end = text[i..].find(['.', '[']).map_or(text.len(), |offset| i + offset);
                    match step(&value, &text[i..end]) {
                        Some(next) => value = next,
                        None => return Ok(None),
                    }
                    i = end;
                }
            }
        }

        Ok(Some(value))
    }
}

fn step(value: &Value, name: &str) -> Option<Value> {
    if let Some(child) = value.as_object().and_then(|object| object.get(name)) {
        return Some(child.clone());
    }
    if name == "length" {
        if let Some(items) = value.as_array() {
            return Some(Value::from(items.len()));
        }
    }
    None
}
