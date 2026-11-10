//! The per-Request rows of a collection run (ADR 0006, ADR 0091). Plain functions over the core's run report, so the
//! list a window shows can be tested without one.

use ricochet_core::collection_runner::RunReport;

/// One Request's line in a run: what it was, whether it passed, and what went wrong if it did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultRow {
    pub label: String,
    pub passed: bool,
    /// The send error, or the first failing check, or empty when everything passed.
    pub detail: String,
}

/// One row per Request in the order the run sent them.
pub fn rows(report: &RunReport) -> Vec<ResultRow> {
    report
        .results
        .iter()
        .map(|result| {
            let label = format!("{} {}", result.method, result.path);
            let detail = match (&result.error, result.assertions.iter().find(|check| !check.passed)) {
                (Some(error), _) => error.clone(),
                (None, Some(check)) => format!("{}: {}", check.description, check.message),
                (None, None) => String::new(),
            };
            ResultRow { label, passed: result.passed(), detail }
        })
        .collect()
}

/// The labels of the Requests that did not pass, in run order.
pub fn failing_labels(report: &RunReport) -> Vec<String> {
    rows(report).into_iter().filter(|row| !row.passed).map(|row| row.label).collect()
}
