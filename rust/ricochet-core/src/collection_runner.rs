//! Port of `src/Ricochet.Core/CollectionRunner.cs` (ADR 0006, 0008, 0020, 0030, 0032). Runs the Requests of one or
//! more Collections in order, checks their assertions and captures, and reports the results. It reads the
//! Workspace and never changes it. Captured values live in memory only, and reset for each Collection.
//!
//! Not ported yet: cookies kept for a run (ADR 0012, the sender has none), and resolving Secret references from
//! external managers (ADR 0025). The xUnit report is in `xunit`, data-row runs are in `data_rows`, and the CLI
//! is in the ricochet-cli crate. The C# app stays the reference for behaviour.

use std::cmp::Reverse;
use std::error::Error;
use std::fmt;

use crate::assertions::{self, AssertionResult};
use crate::folder_tags;
use crate::models::{ApiEnvironment, ApiRequest, Collection, CollectionItem, KeyValue, RequestAuth, Workspace};
use crate::send::RequestSender;
use crate::variables;

/// The text that replaces a secret in output.
const MASK: &str = "***";
/// Shorter values are not masked, because masking them would scramble unrelated output such as a status code.
const MIN_MASKED_LENGTH: usize = 4;

/// The exit code for a run that passed. The C# CLI maps outcomes to these codes in `Program.cs`.
pub const EXIT_PASSED: i32 = 0;
/// The exit code when a Request or a check failed.
pub const EXIT_TESTS_FAILED: i32 = 1;
/// The exit code when the run could not happen at all.
pub const EXIT_CANNOT_RUN: i32 = 2;

/// What was asked for can't be run at all. The message is suitable for showing to the user. Mirrors `RunSetupException`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSetupError {
    message: String,
}

impl RunSetupError {
    fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for RunSetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for RunSetupError {}

/// What to run, and how. Empty filters mean no filter.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunOptions {
    pub collection_name: Option<String>,
    pub folder_name: Option<String>,
    pub environment_name: Option<String>,
    /// Values given for this run. They override every scope, and are treated as Secrets in reports.
    pub overrides: Vec<KeyValue>,
    pub stop_on_first_failure: bool,
    /// Run only Requests with at least one of these tags, which their Folders give them (ADR 0030).
    pub include_tags: Vec<String>,
    /// Skip Requests with any of these tags. Skipping wins over including.
    pub exclude_tags: Vec<String>,
}

/// The outcome of one Request in a run. `captured` names the Variables it set; their values are left out.
#[derive(Debug, Clone, PartialEq)]
pub struct RequestRunResult {
    pub collection: String,
    pub path: String,
    pub method: String,
    pub status_code: Option<u16>,
    pub elapsed_ms: u64,
    pub assertions: Vec<AssertionResult>,
    pub error: Option<String>,
    pub has_scripts: bool,
    pub captured: Vec<String>,
}

impl RequestRunResult {
    /// True when the Request was sent without error and every check passed.
    pub fn passed(&self) -> bool {
        self.error.is_none() && self.assertions.iter().all(|a| a.passed)
    }
}

/// A Request that was selected for the run but left out because it, or a Folder above it, is hidden (ADR 0032).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedRequest {
    pub collection: String,
    pub path: String,
}

/// Everything a run produced. Mirrors `RunReport`.
#[derive(Debug, Clone, PartialEq)]
pub struct RunReport {
    pub results: Vec<RequestRunResult>,
    /// True when the run stopped at the first failure, with Requests still to run.
    pub stopped: bool,
    /// Hidden Requests that would otherwise have run. They are named, so a skipped test can't go unnoticed.
    pub skipped_hidden: Vec<SkippedRequest>,
    /// Set when the sender skipped TLS certificate checks, so no summary calls the run a clean pass (ADR 0079).
    pub certificates_unverified: bool,
}

impl RunReport {
    pub fn passed(&self) -> bool {
        self.results.iter().all(RequestRunResult::passed)
    }

    pub fn failed_requests(&self) -> usize {
        self.results.iter().filter(|r| !r.passed()).count()
    }

    pub fn total_assertions(&self) -> usize {
        self.results.iter().map(|r| r.assertions.len()).sum()
    }

    pub fn failed_assertions(&self) -> usize {
        self.results.iter().flat_map(|r| &r.assertions).filter(|a| !a.passed).count()
    }

    pub fn requests_with_scripts(&self) -> usize {
        self.results.iter().filter(|r| r.has_scripts).count()
    }

    /// Requests that only proved a response came back, because they have no checks or captures.
    pub fn requests_without_assertions(&self) -> usize {
        self.results
            .iter()
            .filter(|r| r.error.is_none() && r.assertions.is_empty() && r.captured.is_empty())
            .count()
    }
}

/// Maps a run's outcome to the CLI's exit code: 0 passed, 1 a Request or check failed, 2 the run could not happen.
pub fn exit_code(outcome: &Result<RunReport, RunSetupError>) -> i32 {
    match outcome {
        Err(_) => EXIT_CANNOT_RUN,
        Ok(report) if report.passed() => EXIT_PASSED,
        Ok(_) => EXIT_TESTS_FAILED,
    }
}

/// One line for the user after a run, such as `2 requests: 1 passed, 1 failed. 3 assertions: 3 passed, 0 failed.`
pub fn summary(report: &RunReport) -> String {
    let mut text = format!(
        "{}: {} passed, {} failed. ",
        count(report.results.len(), "request"),
        report.results.len() - report.failed_requests(),
        report.failed_requests(),
    );
    text.push_str(&format!(
        "{}: {} passed, {} failed.",
        count(report.total_assertions(), "assertion"),
        report.total_assertions() - report.failed_assertions(),
        report.failed_assertions(),
    ));
    if !report.skipped_hidden.is_empty() {
        text.push_str(&format!(" {} skipped because hidden.", count(report.skipped_hidden.len(), "request")));
    }
    if report.certificates_unverified {
        text.push_str(" Certificate checks off.");
    }
    text
}

/// Things that could make a passing run less meaningful than it looks.
pub fn cautions(report: &RunReport) -> Vec<String> {
    let mut cautions = Vec::new();
    if report.stopped {
        cautions.push("Stopped at the first failure, so later requests did not run.".to_string());
    }
    if report.requests_with_scripts() > 0 {
        cautions.push(format!(
            "{} had Postman scripts that were not run, so any tests in them did not count.",
            count(report.requests_with_scripts(), "request"),
        ));
    }
    if report.requests_without_assertions() > 0 {
        cautions.push(format!(
            "{} had no assertions, so only getting a response was checked.",
            count(report.requests_without_assertions(), "request"),
        ));
    }
    if !report.skipped_hidden.is_empty() {
        let names: Vec<String> = report.skipped_hidden.iter().map(|s| format!("{} / {}", s.collection, s.path)).collect();
        cautions.push(format!("Skipped hidden requests, which did not run: {}.", names.join(", ")));
    }
    cautions
}

fn count(n: usize, noun: &str) -> String {
    if n == 1 { format!("1 {noun}") } else { format!("{n} {noun}s") }
}

/// Runs Requests in order and checks their assertions. Reads the Workspace and never changes it.
pub struct CollectionRunner<'a> {
    sender: &'a RequestSender,
}

/// A Request chosen for the run, with the place it sits in the Workspace.
struct Planned<'a> {
    collection_index: usize,
    collection: &'a Collection,
    item_id: &'a str,
    path: String,
    request: &'a ApiRequest,
    has_scripts: bool,
}

#[derive(Default)]
struct Plan<'a> {
    planned: Vec<Planned<'a>>,
    hidden: Vec<SkippedRequest>,
}

impl<'a> CollectionRunner<'a> {
    pub fn new(sender: &'a RequestSender) -> Self {
        Self { sender }
    }

    /// Runs the selected Requests and returns the report.
    ///
    /// # Errors
    /// Returns `RunSetupError` when nothing could be selected to run, or the environment or Collection is not found.
    pub fn run(&self, workspace: &Workspace, options: &RunOptions) -> Result<RunReport, RunSetupError> {
        self.run_with(workspace, options, |_| {})
    }

    /// Like `run`, and calls `on_result` after each Request finishes, so a caller can show progress.
    ///
    /// # Errors
    /// Same as `run`.
    pub fn run_with(
        &self,
        workspace: &Workspace,
        options: &RunOptions,
        mut on_result: impl FnMut(&RequestRunResult),
    ) -> Result<RunReport, RunSetupError> {
        let environment = find_environment(workspace, options.environment_name.as_deref())?;
        let plan = plan(workspace, options)?;

        let mut results: Vec<RequestRunResult> = Vec::new();
        let mut stopped = false;
        let mut captured: Vec<KeyValue> = Vec::new();
        let mut current_collection: Option<usize> = None;
        for item in &plan.planned {
            // Like other Variables, captured values belong to one Collection.
            if current_collection != Some(item.collection_index) {
                captured.clear();
                current_collection = Some(item.collection_index);
            }

            let result = self.run_one(item, workspace, environment, &mut captured, options);
            on_result(&result);
            let passed = result.passed();
            results.push(result);

            if !passed && options.stop_on_first_failure {
                stopped = results.len() < plan.planned.len();
                break;
            }
        }

        Ok(RunReport {
            results,
            stopped,
            skipped_hidden: plan.hidden,
            certificates_unverified: !self.sender.settings().verify_certificates,
        })
    }

    fn run_one(
        &self,
        item: &Planned<'_>,
        workspace: &Workspace,
        environment: Option<&ApiEnvironment>,
        captured: &mut Vec<KeyValue>,
        options: &RunOptions,
    ) -> RequestRunResult {
        // Lowest priority first: globals, the Collection, its Folders, the environment, captures, then the run's overrides.
        let folder = variables::folder_scope(item.collection, Some(item.item_id));
        let environment_variables: &[KeyValue] = match environment {
            Some(environment) => &environment.variables,
            None => &[],
        };
        let layered = variables::layer(&[
            workspace.globals.as_slice(),
            item.collection.variables.as_slice(),
            folder.as_slice(),
            environment_variables,
            captured.as_slice(),
            options.overrides.as_slice(),
        ]);
        let resolved = variables::apply(item.request, &layered);

        let failed = |error: &str, resolved: &ApiRequest, captured: &[KeyValue]| {
            let mask = secrets_to_mask(&layered, &options.overrides, captured, resolved);
            failure(item, mask_in(error, &mask))
        };

        // Sending a literal {{token}} would fail confusingly, and could reach the wrong place.
        let missing = variables::find_placeholders(&resolved);
        if !missing.is_empty() {
            let message = format!("Not sent, because there is no value for: {}", missing.join(", "));
            return failed(&message, &resolved, captured.as_slice());
        }

        match self.sender.send(&resolved) {
            Err(error) => failed(error.message(), &resolved, captured.as_slice()),
            Ok(response) => {
                let snapshot = response.snapshot();
                let capture = assertions::evaluate_captures(resolved.captures.as_deref().unwrap_or(&[]), &snapshot);
                for value in &capture.values {
                    captured.retain(|c| c.key != value.key);
                    captured.push(value.clone());
                }

                // Worked out after capturing, so a new value is masked in this response's own results too.
                let mask = secrets_to_mask(&layered, &options.overrides, captured, &resolved);
                let checks: Vec<AssertionResult> = assertions::evaluate(resolved.assertions.as_deref().unwrap_or(&[]), &snapshot)
                    .into_iter()
                    .chain(capture.failures)
                    .map(|a| AssertionResult {
                        description: mask_in(&a.description, &mask),
                        message: mask_in(&a.message, &mask),
                        ..a
                    })
                    .collect();

                RequestRunResult {
                    collection: item.collection.name.clone(),
                    path: item.path.clone(),
                    method: item.request.method.clone(),
                    status_code: Some(response.status_code),
                    elapsed_ms: u64::try_from(response.elapsed.as_millis()).unwrap_or(u64::MAX),
                    assertions: checks,
                    error: None,
                    has_scripts: item.has_scripts,
                    captured: capture.values.iter().map(|v| v.key.clone()).collect(),
                }
            }
        }
    }
}

/// A Request that was not sent, or failed before a response. Its message is already masked.
fn failure(item: &Planned<'_>, message: String) -> RequestRunResult {
    RequestRunResult {
        collection: item.collection.name.clone(),
        path: item.path.clone(),
        method: item.request.method.clone(),
        status_code: None,
        elapsed_ms: 0,
        assertions: Vec::new(),
        error: Some(message),
        has_scripts: item.has_scripts,
        captured: Vec::new(),
    }
}

fn find_environment<'a>(workspace: &'a Workspace, name: Option<&str>) -> Result<Option<&'a ApiEnvironment>, RunSetupError> {
    let Some(name) = name.filter(|n| !n.is_empty()) else {
        return Ok(None);
    };

    workspace
        .environments
        .iter()
        .find(|e| same_name(&e.name, name))
        .map(Some)
        .ok_or_else(|| {
            RunSetupError::new(format!(
                "No environment is named \"{name}\". Available: {}",
                names_of(workspace.environments.iter().map(|e| e.name.as_str()))
            ))
        })
}

fn plan<'a>(workspace: &'a Workspace, options: &RunOptions) -> Result<Plan<'a>, RunSetupError> {
    let selected: Vec<(usize, &Collection)> = match options.collection_name.as_deref().filter(|n| !n.is_empty()) {
        None => workspace.collections.iter().enumerate().collect(),
        Some(name) => {
            let matched: Vec<(usize, &Collection)> = workspace
                .collections
                .iter()
                .enumerate()
                .filter(|(_, c)| same_name(&c.name, name))
                .collect();
            if matched.is_empty() {
                return Err(RunSetupError::new(format!(
                    "No collection is named \"{name}\". Available: {}",
                    names_of(workspace.collections.iter().map(|c| c.name.as_str()))
                )));
            }
            matched
        }
    };

    let folder_name = options.folder_name.as_deref().filter(|n| !n.is_empty());
    let mut plan = Plan::default();
    for (collection_index, collection) in selected {
        let folder_matched = folder_name.is_none();
        let collection_has_scripts = !collection.scripts.is_empty();
        for item in &collection.items {
            walk(
                collection_index,
                collection,
                item,
                &Walk {
                    parent_path: "",
                    folder_matched,
                    parent_has_scripts: collection_has_scripts,
                    inherited_tags: &[],
                    parent_hidden: false,
                },
                options,
                &mut plan,
            );
        }
    }

    // A run that tests nothing must not look like a success.
    if plan.planned.is_empty() {
        let hidden = plan.hidden.len();
        if hidden > 0 {
            return Err(RunSetupError::new(if hidden == 1 {
                "The only request that would run is hidden. Show it in the app to run it.".to_string()
            } else {
                format!("All {hidden} requests that would run are hidden. Show them in the app to run them.")
            }));
        }
        if let Some(folder) = folder_name {
            return Err(RunSetupError::new(format!("No requests were found in a folder named \"{folder}\".")));
        }
        if !options.include_tags.is_empty() || !options.exclude_tags.is_empty() {
            return Err(RunSetupError::new("No requests match the tags given."));
        }
        return Err(RunSetupError::new("There are no requests to run."));
    }

    Ok(plan)
}

/// What the walk knows about the place it has reached, passed down from the Folders above.
struct Walk<'t> {
    parent_path: &'t str,
    folder_matched: bool,
    parent_has_scripts: bool,
    inherited_tags: &'t [String],
    parent_hidden: bool,
}

fn walk<'a>(
    collection_index: usize,
    collection: &'a Collection,
    item: &'a CollectionItem,
    place: &Walk<'_>,
    options: &RunOptions,
    plan: &mut Plan<'a>,
) {
    // A hidden Folder, and everything in it, is not run (ADR 0032). Hidden Requests that were selected are named instead.
    let is_hidden = place.parent_hidden || collection.hidden_item_ids.as_ref().is_some_and(|ids| ids.contains(&item.id));

    let path = if place.parent_path.is_empty() {
        item.name.clone()
    } else {
        format!("{} / {}", place.parent_path, item.name)
    };
    let has_scripts = place.parent_has_scripts || !item.scripts.is_empty();

    // Requests inherit the tags of every Folder above them (ADR 0030).
    let mut tags: Vec<String> = place.inherited_tags.to_vec();
    tags.extend(item.tags.iter().flatten().cloned());

    if let Some(request) = &item.request {
        if place.folder_matched && folder_tags::selects(place.inherited_tags, &options.include_tags, &options.exclude_tags) {
            if is_hidden {
                plan.hidden.push(SkippedRequest { collection: collection.name.clone(), path });
            } else {
                plan.planned.push(Planned {
                    collection_index,
                    collection,
                    item_id: &item.id,
                    path,
                    request,
                    has_scripts,
                });
            }
        }
        return;
    }

    let matched = place.folder_matched || options.folder_name.as_deref().is_some_and(|f| same_name(&item.name, f));
    let below = Walk {
        parent_path: &path,
        folder_matched: matched,
        parent_has_scripts: has_scripts,
        inherited_tags: &tags,
        parent_hidden: is_hidden,
    };
    for child in &item.children {
        walk(collection_index, collection, child, &below, options, plan);
    }
}

/// The values that must never appear in output: secret Variables, the run's overrides, every captured value
/// (captures are often tokens, and nothing marks which ones), and the secrets of the Request's auth. Values shorter
/// than four characters are left out, and the longest come first, so a value that contains another is masked whole.
pub(crate) fn secrets_to_mask(variables: &[KeyValue], overrides: &[KeyValue], captured: &[KeyValue], resolved: &ApiRequest) -> Vec<String> {
    let mut values: Vec<&str> = variables
        .iter()
        .filter(|v| v.is_secret)
        .map(|v| v.value.as_str())
        .chain(overrides.iter().map(|v| v.value.as_str()))
        .chain(captured.iter().map(|v| v.value.as_str()))
        .collect();
    if let Some(auth) = &resolved.auth {
        values.extend(auth_secret_values(auth));
    }

    let mut kept: Vec<String> = Vec::new();
    for value in values {
        if value.chars().count() >= MIN_MASKED_LENGTH && !kept.iter().any(|k| k == value) {
            kept.push(value.to_string());
        }
    }
    kept.sort_by_key(|v| Reverse(v.chars().count()));
    kept
}

/// The secrets an auth setting holds. Their names are not stored with them, so all are counted.
pub(crate) fn auth_secret_values(auth: &RequestAuth) -> Vec<&str> {
    let mut values = vec![auth.token.as_str(), auth.password.as_str(), auth.key_value.as_str()];
    if let Some(details) = &auth.details {
        values.extend([
            details.consumer_secret.as_str(),
            details.token_secret.as_str(),
            details.aws_secret_access_key.as_str(),
            details.aws_session_token.as_str(),
        ]);
    }
    values
}

/// Replaces each secret in `text` with `***`, longest secret first.
pub(crate) fn mask_in(text: &str, secrets: &[String]) -> String {
    secrets.iter().fold(text.to_string(), |current, secret| current.replace(secret.as_str(), MASK))
}

fn same_name(a: &str, b: &str) -> bool {
    a.to_uppercase() == b.to_uppercase()
}

fn names_of<'a>(names: impl Iterator<Item = &'a str>) -> String {
    let names: Vec<&str> = names.collect();
    if names.is_empty() { "none".to_string() } else { names.join(", ") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masking_drops_short_values_and_repeats_and_masks_the_rest() {
        let secret = KeyValue { is_secret: true, ..KeyValue::new("a", "abcdef") };
        let short = KeyValue { is_secret: true, ..KeyValue::new("b", "abc") };
        let secrets = secrets_to_mask(&[secret, short], &[KeyValue::new("c", "abcdef")], &[], &ApiRequest::default());

        assert_eq!(secrets, vec!["abcdef".to_string()]);
        assert_eq!(mask_in("abcdef and abcdef", &secrets), "*** and ***");
    }
}
