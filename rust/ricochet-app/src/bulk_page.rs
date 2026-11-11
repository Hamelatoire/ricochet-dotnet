//! The bulk run page's logic (ADR 0015, ADR 0017, ADR 0091): sends the Request on screen once for each pasted id, and
//! remembers how far a stopped run got. Plain functions over the core's bulk runner, so tests reach them without a
//! window. A run reports into [`BulkShared`]; the window takes the results from it in turns, follows them with a
//! [`RunTracker`], and saves the tracker's progress in state.json, as the C# bulk page does.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use ricochet_core::bulk_runner::{BulkItemResult, BulkRun, BulkRunner, DEFAULT_PLACEHOLDER, parse_ids, resume_index};
use ricochet_core::models::{KeyValue, Workspace};
use ricochet_core::personal_state::BulkProgress;
use ricochet_core::send::RequestSender;

use crate::edit::collection_id_of;
use crate::tree::find_request;

/// The fewest and most ids sent at once, and the number a new panel starts with, as on the C# bulk page.
pub const MIN_THREADS: usize = 1;
pub const MAX_THREADS: usize = 16;
pub const DEFAULT_THREADS: usize = 4;

/// What a run reports while it goes: the results so far, whether it is done, and a flag the window sets to stop it.
#[derive(Default)]
pub struct BulkShared {
    results: Mutex<Vec<BulkItemResult>>,
    pub done: AtomicBool,
    pub stop: AtomicBool,
}

impl BulkShared {
    /// The results so far in the order the ids were pasted, which is not the order they finished in.
    pub fn results(&self) -> Vec<BulkItemResult> {
        let mut results = self.results.lock().unwrap_or_else(PoisonError::into_inner).clone();
        results.sort_by_key(|result| result.position);
        results
    }

    /// The results that came in after the first `from`, in the order they finished. The window takes them in turns.
    pub fn results_since(&self, from: usize) -> Vec<BulkItemResult> {
        let results = self.results.lock().unwrap_or_else(PoisonError::into_inner);
        results.get(from..).map(<[BulkItemResult]>::to_vec).unwrap_or_default()
    }

    fn push(&self, result: BulkItemResult) {
        self.results.lock().unwrap_or_else(PoisonError::into_inner).push(result);
    }
}

/// Keeps a parallel-sends setting between [`MIN_THREADS`] and [`MAX_THREADS`].
pub fn clamp_threads(threads: i64) -> usize {
    threads.clamp(MIN_THREADS as i64, MAX_THREADS as i64) as usize
}

/// The parallel-sends setting after pressing − (`step` -1) or + (`step` 1), kept within the limits.
pub fn step_threads(current: usize, step: i64) -> usize {
    clamp_threads(current as i64 + step)
}

/// The placeholder the ids replace: the typed name without surrounding spaces, or `id` when the field is blank.
pub fn placeholder_or_default(text: &str) -> &str {
    let trimmed = text.trim();
    if trimmed.is_empty() { DEFAULT_PLACEHOLDER } else { trimmed }
}

/// The most ids put in the id box. A longer list, usually from a file, is kept beside the box instead, because the box
/// lays out every line it holds and would slow the window down. The run sends every id either way.
pub const MAX_IDS_IN_BOX: usize = 1_000;

/// True when a list of this many ids is shown in the id box, false when it is too long and kept beside it.
pub fn fits_in_box(count: usize) -> bool {
    count <= MAX_IDS_IN_BOX
}

/// The ids from a text file, one per line, and a status line naming the file. Blank lines and surrounding spaces are
/// dropped, as with paste. There is no limit on the number of ids.
pub fn load_ids(text: &str, file_name: &str) -> (Vec<String>, String) {
    let ids = parse_ids(text);
    let status =
        if ids.is_empty() { format!("{file_name} has no ids.") } else { format!("Loaded {} id(s) from {file_name}.", ids.len()) };
    (ids, status)
}

/// What the id box shows in place of a list too long for it.
pub fn kept_beside_box(count: usize, source: &str) -> String {
    format!(
        "{count} ids from {source}. They are kept out of this box so the window stays quick, and every one is sent. Remove ids to paste others."
    )
}

/// The status line when the panel opens: what was saved for this Request.
pub fn opening_status(saved: Option<&BulkProgress>) -> String {
    match saved {
        None => "No ids processed yet for this request.".to_string(),
        Some(saved) => {
            format!("Last id processed: {}. {} processed so far.", saved.last_id.as_deref().unwrap_or("none"), saved.processed)
        }
    }
}

/// One line saying what Start, Resume and Stop will do right now.
pub fn action_hint(running: bool, saved: Option<&BulkProgress>) -> String {
    if running {
        return "Running. Stop finishes the ids already in flight, then the run ends.".to_string();
    }
    match saved.and_then(|saved| saved.last_id.as_deref()) {
        Some(last) => format!("Resume continues after {last}. Start sends every id from the first one."),
        None => "Start sends every id from the first one.".to_string(),
    }
}

/// What a run will send: every pasted id, and the position it starts from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPlan {
    pub ids: Vec<String>,
    pub start: usize,
    /// Said before the run when Resume could not find the last id, so it starts from the first.
    pub notice: Option<String>,
}

/// Works out what Start (`resume` false) or Resume sends, from the ids on the panel and the progress saved for the
/// Request. Start always begins at the first id. Resume begins after the saved last id, or at the first when that id
/// is not in the list. An error text says why nothing can run.
pub fn plan_run(ids: Vec<String>, saved: Option<&BulkProgress>, resume: bool) -> Result<RunPlan, String> {
    if ids.is_empty() {
        return Err(if resume { "Paste the ids, then resume." } else { "Paste at least one id." }.to_string());
    }
    let last_id = if resume { saved.and_then(|saved| saved.last_id.as_deref()) } else { None };
    let start = resume_index(&ids, last_id);
    if start >= ids.len() {
        return Err("Every id has already been processed.".to_string());
    }
    let notice = last_id
        .filter(|last| !ids.iter().any(|id| id == last))
        .map(|last| format!("The last id, {last}, is not in this list, so the run starts from the first id."));
    Ok(RunPlan { ids, start, notice })
}

/// What Start saves before it sends: the ids with no last id, so a Resume after it begins from the first.
pub fn starting_progress(request_id: &str, ids: &[String]) -> BulkProgress {
    BulkProgress { request_id: request_id.to_string(), ids: ids.join("\n"), last_id: None, processed: 0 }
}

/// The line that opens a run in the log and in `bulk-log.txt`.
pub fn run_header(first_id: &str, threads: usize, count: usize, now: &str) -> String {
    format!("--- Run from {first_id}, {threads} thread(s), {count} id(s), {now} ---")
}

/// The status line as a run starts.
pub fn starting_status(first_id: &str, threads: usize) -> String {
    format!("Running from {first_id} on {threads} {}…", if threads == 1 { "thread" } else { "threads" })
}

/// Follows a run as results arrive, in whatever order they finish, and says what to save and show.
///
/// Ids can finish out of order, so the last id saved is the end of the leading run of finished ids. Resume starts
/// after it, which sends again any later id that finished early. That is safe, and nothing is skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunTracker {
    request_id: String,
    ids: Vec<String>,
    start: usize,
    finished: Vec<bool>,
    frontier: usize,
    processed: usize,
    calls_sent: usize,
    calls_ok: usize,
}

impl RunTracker {
    /// A run of `ids` from position `start`. The ids before it count as processed.
    pub fn new(request_id: &str, ids: Vec<String>, start: usize) -> Self {
        let start = start.min(ids.len());
        Self {
            request_id: request_id.to_string(),
            finished: vec![false; ids.len()],
            ids,
            start,
            frontier: start,
            processed: start,
            calls_sent: 0,
            calls_ok: 0,
        }
    }

    /// Notes one result. Its position counts from the run's start, as the core runner reports it.
    pub fn record(&mut self, result: &BulkItemResult) {
        let Some(flag) = self.finished.get_mut(self.start + result.position) else {
            return;
        };
        if *flag {
            return;
        }
        *flag = true;
        self.processed += 1;
        if result.status_code.is_some() {
            self.calls_sent += 1;
        }
        if result.succeeded {
            self.calls_ok += 1;
        }
        while self.frontier < self.ids.len() && self.finished[self.frontier] {
            self.frontier += 1;
        }
    }

    /// The last id of the leading run of finished ids, where Resume continues. None before the first one finishes.
    pub fn last_id(&self) -> Option<&str> {
        self.frontier.checked_sub(1).map(|index| self.ids[index].as_str())
    }

    pub fn processed(&self) -> usize {
        self.processed
    }

    pub fn total(&self) -> usize {
        self.ids.len()
    }

    /// What is saved in state.json for the Request.
    pub fn progress(&self) -> BulkProgress {
        BulkProgress {
            request_id: self.request_id.clone(),
            ids: self.ids.join("\n"),
            last_id: self.last_id().map(str::to_string),
            processed: self.processed as i32,
        }
    }

    /// The status line while the run goes.
    pub fn running_status(&self) -> String {
        format!("{} of {} processed. Last id: {}. {}", self.processed, self.ids.len(), self.last_id().unwrap_or("none"), self.calls_text())
    }

    /// The status line when the run ends: every id was sent (`finished`), or Stop ended it first.
    pub fn ended_status(&self, finished: bool) -> String {
        if finished {
            format!("Finished. {} of {} processed. {}", self.processed, self.ids.len(), self.calls_text())
        } else {
            format!("Stopped after {}. Resume to continue from there. {}", self.last_id().unwrap_or("nothing"), self.calls_text())
        }
    }

    /// Calls actually sent in this run, and how many got a 2xx. Ids that were not sent don't count.
    fn calls_text(&self) -> String {
        format!("Responses: {} ({} OK, {} failed).", self.calls_sent, self.calls_ok, self.calls_sent - self.calls_ok)
    }
}

/// Sends the Request in the sidebar item `item_id` once for each id, replacing `{{id}}` (or `placeholder`), at most
/// `max_parallel` at a time. `overrides` are the Workspace's `.env` Variables. Results are pushed to `shared` as they
/// come, and `shared.done` is set when the run ends. Returns true when every id was sent, false when the run was
/// stopped first, and an error text when the run could not start.
#[allow(clippy::too_many_arguments)]
pub fn run(
    workspace: &Workspace,
    item_id: &str,
    environment_id: Option<&str>,
    ids: &[String],
    placeholder: &str,
    max_parallel: usize,
    overrides: &[KeyValue],
    sender: &RequestSender,
    shared: &BulkShared,
) -> Result<bool, String> {
    let outcome = run_inner(workspace, item_id, environment_id, ids, placeholder, max_parallel, overrides, sender, shared);
    shared.done.store(true, Ordering::SeqCst);
    outcome
}

#[allow(clippy::too_many_arguments)]
fn run_inner(
    workspace: &Workspace,
    item_id: &str,
    environment_id: Option<&str>,
    ids: &[String],
    placeholder: &str,
    max_parallel: usize,
    overrides: &[KeyValue],
    sender: &RequestSender,
    shared: &BulkShared,
) -> Result<bool, String> {
    let template = find_request(workspace, item_id).ok_or("Pick a request to run it for each id.")?;
    let collection_id = collection_id_of(workspace, item_id).ok_or("The request is not in a collection.")?;
    let collection = workspace.collections.iter().find(|collection| collection.id == collection_id).ok_or("The collection is missing.")?;
    let environment = environment_id.and_then(|id| workspace.environments.iter().find(|environment| environment.id == id));

    let bulk = BulkRun {
        workspace,
        collection,
        item_id: Some(item_id),
        environment,
        template,
        placeholder: placeholder_or_default(placeholder),
        ids,
        overrides,
        max_parallel: clamp_threads(max_parallel as i64),
    };
    BulkRunner::new(sender).run(&bulk, &shared.stop, |result| shared.push(result)).map_err(|error| error.to_string())
}
