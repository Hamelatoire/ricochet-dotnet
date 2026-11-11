//! Port of `src/Ricochet.Core/BulkRunner.cs` (ADR 0015, 0017). Sends one Request for each pasted id, with the id
//! replacing a placeholder such as `{{id}}`. Ids start in the pasted order, at most `max_parallel` at a time, and
//! results arrive in completion order, one at a time. Nothing here stores progress: the caller records each
//! result, and uses `resume_index` to continue a run.
//!
//! The progress numbers (`BulkRunProgress.cs`) are in `bulk_run_progress`. Secret references from external
//! managers (ADR 0025) are read just before each send. Stopping is a shared flag, not a cancellation token. The C# app
//! stays the reference.

use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use std::thread;

use crate::collection_runner::{mask_in, secrets_to_mask};
use crate::external_secrets::ExternalSecretResolver;
use crate::models::{ApiEnvironment, ApiRequest, Collection, KeyValue, Workspace};
use crate::send::RequestSender;
use crate::variables;

/// The placeholder an id replaces when the Request does not name another.
pub const DEFAULT_PLACEHOLDER: &str = "id";

/// The most characters of a response body kept for a failed id's detail.
const MAX_DETAIL_LENGTH: usize = 300;

/// The outcome of sending the Request for one id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkItemResult {
    pub id: String,
    /// None when nothing was sent, or the request failed before a response came back.
    pub status_code: Option<u16>,
    /// True only for a 2xx response.
    pub succeeded: bool,
    /// The start of the response body for a failed id, or the error. Secrets are masked.
    pub detail: String,
    pub elapsed_ms: u64,
    /// The id's place in the pasted list, so a caller can tell which ids have finished when they complete out of order.
    pub position: usize,
}

impl BulkItemResult {
    /// One line for the log: OK or ERR, the id, the status and the start of the response.
    pub fn log_line(&self) -> String {
        let status = self.status_code.map_or_else(|| "-".to_string(), |s| s.to_string());
        let marker = if self.succeeded { "OK " } else { "ERR" };
        format!("{marker} {}  [{status}] {}", self.id, self.detail).trim_end().to_string()
    }
}

/// Why the run cannot start. Nothing has been sent when one of these is returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BulkRunError {
    /// It was asked for no threads.
    NoThreads,
    /// The Request never uses the placeholder, so every id would send the same request.
    PlaceholderUnused(String),
}

impl fmt::Display for BulkRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoThreads => f.write_str("At least one thread is needed."),
            Self::PlaceholderUnused(placeholder) => write!(
                f,
                "Not started: the request doesn't use {{{{{placeholder}}}}}, so every id would send the same request. \
                 Put {{{{{placeholder}}}}} in the URL, body or headers, or set the placeholder to the variable it uses."
            ),
        }
    }
}

impl Error for BulkRunError {}

/// Splits pasted text into ids: one per line, with blank lines and surrounding spaces dropped.
pub fn parse_ids(text: &str) -> Vec<String> {
    text.split(['\r', '\n']).map(str::trim).filter(|id| !id.is_empty()).map(str::to_string).collect()
}

/// The position a run resumes from: the one after `last_id` in the pasted list. Zero when there is no last id, or
/// it is no longer in the list, so the run starts from the first id.
pub fn resume_index(ids: &[String], last_id: Option<&str>) -> usize {
    let Some(last_id) = last_id else { return 0 };
    ids.iter().position(|id| id == last_id).map_or(0, |index| index + 1)
}

/// What one bulk run sends: the Request as a template, the Collection it belongs to, and the ids.
#[derive(Debug, Clone, Copy)]
pub struct BulkRun<'a> {
    pub workspace: &'a Workspace,
    pub collection: &'a Collection,
    /// The item the template is, so the Folders around it contribute their Variables (ADR 0020).
    pub item_id: Option<&'a str>,
    pub environment: Option<&'a ApiEnvironment>,
    pub template: &'a ApiRequest,
    pub placeholder: &'a str,
    pub ids: &'a [String],
    pub overrides: &'a [KeyValue],
    /// How many ids are sent at once. One sends them in turn (ADR 0017).
    pub max_parallel: usize,
}

/// Sends the Request for each id. Holds only the sender and the Secret reference reader, and keeps no progress.
pub struct BulkRunner<'a> {
    sender: &'a RequestSender,
    /// Reads external Secret references just before each send (ADR 0025).
    secrets: ExternalSecretResolver,
}

impl<'a> BulkRunner<'a> {
    /// A runner that reads Secret references from the process environment and the system shell.
    pub fn new(sender: &'a RequestSender) -> Self {
        Self::with_secrets(sender, ExternalSecretResolver::system())
    }

    /// A runner that reads Secret references with `secrets`, so tests can give fixed values.
    pub fn with_secrets(sender: &'a RequestSender, secrets: ExternalSecretResolver) -> Self {
        Self { sender, secrets }
    }

    /// Runs the Request for each id. Calls `on_result` for each result, one at a time, in completion order. The
    /// callback runs on the worker that finished the id, before that worker takes its next id, so a stop set in
    /// the callback starts nothing new.
    ///
    /// Once `stop` is set, no new id starts, and the ids already in flight still finish, so none is left half sent.
    /// Returns true when every id was sent, and false when the run was stopped first.
    ///
    /// # Errors
    /// Returns `BulkRunError::NoThreads` when `max_parallel` is zero, and `BulkRunError::PlaceholderUnused` when the
    /// id would change nothing in the request, such as a placeholder of `id` for a URL that uses `{{productId}}`.
    pub fn run<F>(&self, run: &BulkRun<'_>, stop: &AtomicBool, on_result: F) -> Result<bool, BulkRunError>
    where
        F: FnMut(BulkItemResult) + Send,
    {
        if run.max_parallel < 1 {
            return Err(BulkRunError::NoThreads);
        }

        let folder_scope = variables::folder_scope(run.collection, run.item_id);
        // Two different ids must give two different requests. Checked through every scope, so a placeholder reached
        // through another Variable, such as productId = {{id}}, counts as used.
        let first = variables::apply(run.template, &Self::scope_for(run, &folder_scope, "ricochet-bulk-check-a"));
        let second = variables::apply(run.template, &Self::scope_for(run, &folder_scope, "ricochet-bulk-check-b"));
        if first == second {
            return Err(BulkRunError::PlaceholderUnused(run.placeholder.to_string()));
        }
        let next = AtomicUsize::new(0);
        let sent = AtomicUsize::new(0);
        let on_result = Mutex::new(on_result);
        let workers = run.max_parallel.min(run.ids.len());

        thread::scope(|scope| {
            for _ in 0..workers {
                let next = &next;
                let sent = &sent;
                let on_result = &on_result;
                let folder_scope = folder_scope.as_slice();
                scope.spawn(move || {
                    loop {
                        if stop.load(Ordering::SeqCst) {
                            break;
                        }
                        let position = next.fetch_add(1, Ordering::SeqCst);
                        // Checked again after the id is claimed, so a stop that came in meanwhile starts nothing new.
                        if position >= run.ids.len() || stop.load(Ordering::SeqCst) {
                            break;
                        }
                        let result = self.run_one(run, folder_scope, &run.ids[position], position);
                        // One result at a time, so the log and the caller's progress are never interleaved.
                        let mut callback = on_result.lock().unwrap_or_else(PoisonError::into_inner);
                        callback(result);
                        drop(callback);
                        sent.fetch_add(1, Ordering::SeqCst);
                    }
                });
            }
        });

        Ok(sent.load(Ordering::SeqCst) == run.ids.len())
    }

    /// Every scope for one id, lowest priority first, with the id last so it wins.
    fn scope_for(run: &BulkRun<'_>, folder_scope: &[KeyValue], id: &str) -> Vec<KeyValue> {
        let placeholder = [KeyValue::new(run.placeholder, id)];
        let environment_variables: &[KeyValue] = match run.environment {
            Some(environment) => &environment.variables,
            None => &[],
        };
        variables::layer(&[
            run.workspace.globals.as_slice(),
            run.collection.variables.as_slice(),
            folder_scope,
            environment_variables,
            run.overrides,
            placeholder.as_slice(),
        ])
    }

    fn run_one(&self, run: &BulkRun<'_>, folder_scope: &[KeyValue], id: &str, position: usize) -> BulkItemResult {
        let layered = Self::scope_for(run, folder_scope, id);
        // Secret references are read just before sending, and only the send uses the values (ADR 0025).
        let layered = match variables::resolve_references(run.template, &layered, &self.secrets) {
            Ok(with_references) => with_references,
            Err(message) => {
                return BulkItemResult {
                    id: id.to_string(),
                    status_code: None,
                    succeeded: false,
                    detail: message,
                    elapsed_ms: 0,
                    position,
                };
            }
        };
        let resolved = variables::apply(run.template, &layered);
        let secrets = secrets_to_mask(&layered, run.overrides, &[], &resolved);

        // Sending a literal {{name}} could reach the wrong place, so an unresolved one stops this id only.
        let missing = variables::find_placeholders(&resolved);
        if !missing.is_empty() {
            return BulkItemResult {
                id: id.to_string(),
                status_code: None,
                succeeded: false,
                detail: format!("Not sent, because there is no value for: {}", missing.join(", ")),
                elapsed_ms: 0,
                position,
            };
        }

        match self.sender.send(&resolved) {
            Ok(response) => {
                let succeeded = (200..300).contains(&response.status_code);
                let detail = if succeeded { String::new() } else { snippet(&response.body_text()) };
                BulkItemResult {
                    id: id.to_string(),
                    status_code: Some(response.status_code),
                    succeeded,
                    detail: mask_in(&detail, &secrets),
                    elapsed_ms: u64::try_from(response.elapsed.as_millis()).unwrap_or(u64::MAX),
                    position,
                }
            }
            Err(error) => BulkItemResult {
                id: id.to_string(),
                status_code: None,
                succeeded: false,
                detail: mask_in(error.message(), &secrets),
                elapsed_ms: 0,
                position,
            },
        }
    }
}

/// The start of a response body on one line, cut to `MAX_DETAIL_LENGTH` characters.
fn snippet(text: &str) -> String {
    let flat = text.replace("\r\n", " ").replace(['\r', '\n'], " ");
    let flat = flat.trim();
    if flat.chars().count() <= MAX_DETAIL_LENGTH {
        flat.to_string()
    } else {
        let cut: String = flat.chars().take(MAX_DETAIL_LENGTH).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_line_uses_dash_for_a_missing_status_and_drops_trailing_space() {
        let failed = BulkItemResult {
            id: "p9".into(),
            status_code: None,
            succeeded: false,
            detail: "connection refused".into(),
            elapsed_ms: 0,
            position: 0,
        };
        assert_eq!(failed.log_line(), "ERR p9  [-] connection refused");

        let ok = BulkItemResult { status_code: Some(200), succeeded: true, detail: String::new(), ..failed };
        assert_eq!(ok.log_line(), "OK  p9  [200]");
    }
}
