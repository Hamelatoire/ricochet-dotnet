//! Port of `RunReports.WriteXunit` in src/Ricochet.Core/CollectionRunner.cs (ADR 0018). Writes xUnit's v2 XML
//! format, with one assembly per Collection and one test per Request. The text is already masked by the runner
//! (ADR 0076), so nothing here adds Secret values.

use std::fmt::Write;

use chrono::{DateTime, Local};

use crate::collection_runner::{RequestRunResult, RunReport};

/// Builds the XML report for a run. `started_at` is the time the run began, as the C# CLI records it.
pub fn write_xunit(report: &RunReport, started_at: DateTime<Local>) -> String {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    let _ = writeln!(xml, "<assemblies timestamp=\"{}\">", attr(&started_at.format("%m/%d/%Y %H:%M:%S").to_string()));

    for (name, suite) in group_by_collection(&report.results) {
        let name = clean(name);
        let time_ms: u64 = suite.iter().map(|r| r.elapsed_ms).sum();
        let total = suite.len();
        let passed = suite.iter().filter(|r| r.passed()).count();
        let failed = total - passed;
        let time = seconds(time_ms);

        let _ = writeln!(
            xml,
            "  <assembly name=\"{}\" test-framework=\"Ricochet\" run-date=\"{}\" run-time=\"{}\" time=\"{}\" total=\"{total}\" passed=\"{passed}\" failed=\"{failed}\" skipped=\"0\" errors=\"0\">",
            attr(&name),
            started_at.format("%Y-%m-%d"),
            started_at.format("%H:%M:%S"),
            attr(&time),
        );
        xml.push_str("    <errors></errors>\n");

        // Written only when checks were off, so a report viewer can flag the run. Readers ignore unknown properties.
        if report.certificates_unverified {
            xml.push_str("    <properties>\n      <property name=\"certificate-checks\" value=\"off\" />\n    </properties>\n");
        }

        let _ = writeln!(
            xml,
            "    <collection name=\"{0}\" time=\"{1}\" total=\"{total}\" passed=\"{passed}\" failed=\"{failed}\" skipped=\"0\">",
            attr(&name),
            attr(&time),
        );

        for result in suite {
            write_test(&mut xml, result, &name);
        }

        xml.push_str("    </collection>\n  </assembly>\n");
    }

    xml.push_str("</assemblies>\n");
    xml
}

fn write_test(xml: &mut String, result: &RequestRunResult, suite_name: &str) {
    let test_name = clean(&format!("{} {}", result.method, result.path));
    let outcome = if result.passed() { "Pass" } else { "Fail" };
    let open = format!(
        "<test name=\"{0}\" type=\"{1}\" method=\"{0}\" time=\"{2}\" result=\"{outcome}\"",
        attr(&test_name),
        attr(suite_name),
        attr(&seconds(result.elapsed_ms)),
    );

    if result.passed() {
        let _ = writeln!(xml, "      {open} />");
        return;
    }

    let (exception_type, problems) = match &result.error {
        Some(error) => ("RequestFailed", error.clone()),
        None => (
            "AssertionFailed",
            result
                .assertions
                .iter()
                .filter(|a| !a.passed)
                .map(|a| format!("{}: {}", a.description, a.message))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    };

    // Response text can hold characters XML can't represent, even escaped, so they are replaced first.
    let message = clean(&problems).replace("]]>", "]]&gt;");
    let _ = writeln!(xml, "      {open}>");
    let _ = writeln!(xml, "        <failure exception-type=\"{exception_type}\">");
    let _ = writeln!(xml, "          <message><![CDATA[{message}]]></message>");
    xml.push_str("        </failure>\n      </test>\n");
}

/// Groups results by Collection, in the order each Collection first appears.
fn group_by_collection(results: &[RequestRunResult]) -> Vec<(&str, Vec<&RequestRunResult>)> {
    let mut groups: Vec<(&str, Vec<&RequestRunResult>)> = Vec::new();
    for result in results {
        match groups.iter_mut().find(|(name, _)| *name == result.collection) {
            Some((_, members)) => members.push(result),
            None => groups.push((result.collection.as_str(), vec![result])),
        }
    }
    groups
}

/// Milliseconds as seconds with up to three decimals, without trailing zeros, as the C# `0.###` format gives.
fn seconds(milliseconds: u64) -> String {
    let text = format!("{:.3}", milliseconds as f64 / 1000.0);
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Escapes text for an XML attribute, after removing characters XML can't hold.
fn attr(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in clean(text).chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            '\n' => escaped.push_str("&#xA;"),
            '\r' => escaped.push_str("&#xD;"),
            '\t' => escaped.push_str("&#x9;"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// Replaces each character XML 1.0 can't represent with U+FFFD, as the C# report does.
fn clean(text: &str) -> String {
    text.chars().map(|c| if is_xml_char(c) { c } else { '\u{FFFD}' }).collect()
}

fn is_xml_char(c: char) -> bool {
    matches!(c as u32, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x1_0000..=0x10_FFFF)
}
