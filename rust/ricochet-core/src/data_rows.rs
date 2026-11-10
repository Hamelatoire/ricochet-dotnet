//! Port of src/Ricochet.Core/DataRowRunner.cs and DataRowReports.cs (ADR 0018). Runs a Collection once per row of a
//! data file. Each row's values are given to its run as overrides, so every run starts fresh: captures and results
//! never carry over from one row to the next. The Workspace is read and never changed.

use crate::collection_runner::{CollectionRunner, RequestRunResult, RunOptions, RunReport, RunSetupError};
use crate::data_file::Row;
use crate::models::Workspace;
use crate::send::RequestSender;

/// The report for one row of a data file, from the run made with that row's values.
#[derive(Debug, Clone, PartialEq)]
pub struct DataRowReport {
    /// The row's place in the data file, counted from 1 over the rows that hold values.
    pub row: usize,
    pub report: RunReport,
}

/// Runs the Collection once per row. `on_row` is called after each row finishes. Stop on first failure stops the
/// whole data run, not just the row that failed.
///
/// # Errors
/// Returns `RunSetupError` when a run cannot be set up, such as an unknown Collection or environment name.
pub fn run_data_rows(
    sender: &RequestSender,
    workspace: &Workspace,
    options: &RunOptions,
    rows: &[Row],
    mut on_row: impl FnMut(&DataRowReport),
) -> Result<Vec<DataRowReport>, RunSetupError> {
    let runner = CollectionRunner::new(sender);
    let mut reports = Vec::new();

    for (index, row) in rows.iter().enumerate() {
        // Row values come after the given overrides, so a row wins when both name the same Variable.
        let mut row_options = options.clone();
        row_options.overrides.extend(row.iter().cloned());
        let report = runner.run(workspace, &row_options)?;
        let passed = report.passed();

        let entry = DataRowReport { row: index + 1, report };
        on_row(&entry);
        reports.push(entry);

        if options.stop_on_first_failure && !passed {
            break;
        }
    }

    Ok(reports)
}

/// Joins the reports of a data run into one report, so one xUnit file and one exit code can cover every row.
/// Each Request's Collection gets its row number, so the same Request in two rows stays two results. The combined
/// report is stopped when any row stopped, or when `stopped_early` says the data run did.
pub fn combine(rows: &[DataRowReport], stopped_early: bool) -> RunReport {
    let results: Vec<RequestRunResult> = rows
        .iter()
        .flat_map(|row| {
            row.report.results.iter().map(move |result| RequestRunResult {
                collection: format!("{} (row {})", result.collection, row.row),
                ..result.clone()
            })
        })
        .collect();

    RunReport {
        results,
        stopped: stopped_early || rows.iter().any(|row| row.report.stopped),
        skipped_hidden: Vec::new(),
        certificates_unverified: false,
    }
}
