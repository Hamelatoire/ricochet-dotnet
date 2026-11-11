//! The bulk run page's logic (ADR 0015, ADR 0017, ADR 0091): sends the Request on screen once for each pasted id, and
//! remembers how far a stopped run got. Plain functions over the core's bulk runner, so tests reach them without a
//! window. The window shows the progress this module keeps in [`BulkShared`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use ricochet_core::bulk_runner::{BulkItemResult, BulkRun, BulkRunner, DEFAULT_PLACEHOLDER};
use ricochet_core::models::Workspace;
use ricochet_core::send::RequestSender;

use crate::edit::collection_id_of;
use crate::tree::find_request;

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

    fn push(&self, result: BulkItemResult) {
        self.results.lock().unwrap_or_else(PoisonError::into_inner).push(result);
    }
}

/// How many of the first ids have finished with none missing before them. A stopped run resumes after this many,
/// because ids that finished out of order beyond a gap are sent again rather than trusted.
pub fn finished_prefix(results: &[BulkItemResult]) -> usize {
    let mut positions: Vec<usize> = results.iter().map(|result| result.position).collect();
    positions.sort_unstable();
    positions.into_iter().enumerate().take_while(|(index, position)| index == position).count()
}

/// How many ids failed so far.
pub fn failed(results: &[BulkItemResult]) -> usize {
    results.iter().filter(|result| !result.succeeded).count()
}

/// Sends the Request in the sidebar item `item_id` once for each id, replacing `{{id}}` (or `placeholder`). Results are
/// pushed to `shared` as they come, and `shared.done` is set when the run ends. Returns true when every id was sent,
/// false when the run was stopped first, and an error text when the run could not start.
pub fn run(
    workspace: &Workspace,
    item_id: &str,
    environment_id: Option<&str>,
    ids: &[String],
    placeholder: &str,
    max_parallel: usize,
    sender: &RequestSender,
    shared: &BulkShared,
) -> Result<bool, String> {
    let outcome = run_inner(workspace, item_id, environment_id, ids, placeholder, max_parallel, sender, shared);
    shared.done.store(true, Ordering::SeqCst);
    outcome
}

fn run_inner(
    workspace: &Workspace,
    item_id: &str,
    environment_id: Option<&str>,
    ids: &[String],
    placeholder: &str,
    max_parallel: usize,
    sender: &RequestSender,
    shared: &BulkShared,
) -> Result<bool, String> {
    let template = find_request(workspace, item_id).ok_or("Pick a request to run it for each id.")?;
    let collection_id = collection_id_of(workspace, item_id).ok_or("The request is not in a collection.")?;
    let collection = workspace.collections.iter().find(|collection| collection.id == collection_id).ok_or("The collection is missing.")?;
    let environment = environment_id.and_then(|id| workspace.environments.iter().find(|environment| environment.id == id));
    let placeholder = if placeholder.trim().is_empty() { DEFAULT_PLACEHOLDER } else { placeholder.trim() };

    let bulk = BulkRun {
        workspace,
        collection,
        item_id: Some(item_id),
        environment,
        template,
        placeholder,
        ids,
        overrides: &[],
        max_parallel,
    };
    BulkRunner::new(sender).run(&bulk, &shared.stop, |result| shared.push(result)).map_err(|error| error.to_string())
}
