//! Port of `PostmanTestTranslator.cs` (ADR 0037). Turns simple Postman test checks into Ricochet assertions on
//! import.
//!
//! A script is translated only when every line of it is a recognised check. Otherwise nothing from it is
//! translated, so a partly understood test never looks fully converted, and the importer keeps the script as it
//! always has. Scripts are never run.
//!
//! Recognised checks, each on its own line, or inside `pm.test("name", function () { ... });` or an arrow function:
//!
//! ```text
//! pm.response.to.have.status(200)                          status equals 200
//! pm.expect(pm.response.code).to.eql(200)                  status equals 200 (equal, eq and be.equal also work)
//! pm.expect(pm.response.code === 200).to.be.true           status equals 200
//! pm.expect(pm.response.responseTime).to.be.below(300)     time is less than 300 ms (lessThan also works)
//! pm.response.to.have.header("X-Id")                       header exists
//! pm.response.to.have.header("X-Id", "abc")                header equals abc
//! pm.expect(pm.response.json().items[0].id).to.eql("x")    JSON value at items[0].id equals "x"
//! const data = pm.response.json();                         then pm.expect(data.items[0].id).to.eql(1)
//! ```
//!
//! JSON values are compared as text in Ricochet, so the string "1" also matches the number 1, where Postman's
//! eql does not.

use std::sync::OnceLock;

use regex::Regex;

use crate::assertions::{RequestAssertion, ops, sources};

/// Longest line shown in a reason, in characters. Longer lines are cut and followed by an ellipsis.
const MAX_SHOWN_LENGTH: usize = 100;

macro_rules! pattern {
    ($name:ident, $text:expr) => {
        fn $name() -> &'static Regex {
            static PATTERN: OnceLock<Regex> = OnceLock::new();
            PATTERN.get_or_init(|| Regex::new($text).expect("valid pattern"))
        }
    };
}

pattern!(
    test_start,
    r#"^pm\.test\(\s*(?:"[^"\\]*"|'[^'\\]*')\s*,\s*(?:function\s*\(\s*\)|\(\s*\)\s*=>)\s*\{$"#
);
pattern!(test_end, r#"^\}\)$"#);
pattern!(
    json_variable,
    r#"^(?:const|let|var)\s+([A-Za-z_]\w*)\s*=\s*pm\.response\.json\(\)$"#
);
pattern!(has_status, r#"^pm\.response\.to\.have\.status\((\d{3})\)$"#);
pattern!(
    code_equals,
    r#"^pm\.expect\(pm\.response\.code\)\.to\.(?:be\.)?(?:eql|equal|eq)\((\d{3})\)$"#
);
pattern!(
    code_is_true,
    r#"^pm\.expect\(pm\.response\.code === (\d{3})\)\.to\.be\.true$"#
);
pattern!(
    time_below,
    r#"^pm\.expect\(pm\.response\.responseTime\)\.to\.(?:be\.)?(?:below|lessThan)\((\d+)\)$"#
);
pattern!(
    has_header,
    r#"^pm\.response\.to\.have\.header\(("[^"\\]*"|'[^'\\]*')(?:,\s*("[^"\\]*"|'[^'\\]*'))?\)$"#
);
pattern!(
    json_equals,
    r#"^pm\.expect\((?:pm\.response\.json\(\)|([A-Za-z_]\w*))\.([A-Za-z_]\w*(?:\.[A-Za-z_]\w*|\[\d+\])*)\)\.to\.(?:be\.)?(?:eql|equal|eq)\(("[^"\\]*"|'[^'\\]*'|-?\d+(?:\.\d+)?)\)$"#
);

/// Translates a Request's test script into assertions.
///
/// Returns every assertion in order when every line is a recognised check. Otherwise returns `Err` with the
/// first line that was not recognised (shortened), or the reason the script as a whole cannot be translated.
pub fn try_translate(script: &str) -> Result<Vec<RequestAssertion>, String> {
    let mut assertions = Vec::new();
    let mut json_var: Option<String> = None;
    let mut in_test = false;

    for raw in script.split('\n') {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }

        let statement = match line.strip_suffix(';') {
            Some(rest) => rest.trim_end(),
            None => line,
        };

        if !in_test && test_start().is_match(statement) {
            in_test = true;
        } else if in_test && test_end().is_match(statement) {
            in_test = false;
        } else if let Some(captures) = json_variable().captures(statement) {
            json_var = Some(captures[1].to_string());
        } else if let Some(assertion) = check(statement, json_var.as_deref()) {
            assertions.push(assertion);
        } else {
            return Err(shorten(line));
        }
    }

    if in_test {
        return Err("pm.test without its closing });".to_string());
    }
    if assertions.is_empty() {
        return Err("no checks in it".to_string());
    }

    Ok(assertions)
}

fn check(statement: &str, json_var: Option<&str>) -> Option<RequestAssertion> {
    if let Some(captures) = has_status().captures(statement) {
        return Some(status(&captures[1]));
    }
    if let Some(captures) = code_equals().captures(statement) {
        return Some(status(&captures[1]));
    }
    if let Some(captures) = code_is_true().captures(statement) {
        return Some(status(&captures[1]));
    }

    if let Some(captures) = time_below().captures(statement) {
        return Some(RequestAssertion {
            source: sources::TIME.to_string(),
            op: ops::LESS_THAN.to_string(),
            expected: captures[1].to_string(),
            ..Default::default()
        });
    }

    if let Some(captures) = has_header().captures(statement) {
        let name = unquote(&captures[1]);
        return Some(match captures.get(2) {
            Some(value) => RequestAssertion {
                source: sources::HEADER.to_string(),
                path: name.to_string(),
                op: ops::EQUAL.to_string(),
                expected: unquote(value.as_str()).to_string(),
                ..Default::default()
            },
            None => RequestAssertion {
                source: sources::HEADER.to_string(),
                path: name.to_string(),
                op: ops::EXISTS.to_string(),
                ..Default::default()
            },
        });
    }

    if let Some(captures) = json_equals().captures(statement) {
        // A variable other than the one declared from pm.response.json() is not something we can follow.
        if let Some(variable) = captures.get(1)
            && Some(variable.as_str()) != json_var
        {
            return None;
        }

        return Some(RequestAssertion {
            source: sources::JSON.to_string(),
            path: captures[2].to_string(),
            op: ops::EQUAL.to_string(),
            expected: unquote(&captures[3]).to_string(),
            ..Default::default()
        });
    }

    None
}

fn status(code: &str) -> RequestAssertion {
    RequestAssertion {
        expected: code.to_string(),
        ..Default::default()
    }
}

fn unquote(text: &str) -> &str {
    if text.len() >= 2 && (text.starts_with('"') || text.starts_with('\'')) {
        &text[1..text.len() - 1]
    } else {
        text
    }
}

fn shorten(text: &str) -> String {
    if text.chars().count() <= MAX_SHOWN_LENGTH {
        text.to_string()
    } else {
        let head: String = text.chars().take(MAX_SHOWN_LENGTH).collect();
        format!("{head}…")
    }
}
