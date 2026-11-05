---
status: accepted
---

# Send one Request for each pasted id, and resume after the last one

Some work means sending the same Request once per item, such as syncing each product in a list. Doing it by hand, or with a script outside the app, loses the request's variables and environment and leaves no record of which items finished. We will add a bulk run: the user pastes a list of ids, and the Request on screen is sent once for each, one at a time. Each id replaces a placeholder, `{{id}}` by default, in the URL, body or headers.

## Decisions within this

- **One id at a time:** requests run in the order pasted, and the next one waits for the last to finish. Sending in parallel would be faster, but it could overload the API and makes the log harder to follow.
- **The Request on screen is the template:** unsaved edits are used, as with Send. Variables, the active Environment and the Collection's variables apply as they do for any request.
- **Progress is remembered per Request:** after each id, the last id processed and the pasted list are saved to personal state, so Resume continues after that id even after the app restarts. Starting over clears it. A Request that has never been saved keeps no progress.
- **Stop finishes the current id:** cancelling never leaves an id half sent, so the last id processed is always one that got a response or an error.
- **An id is OK only for a 2xx response:** anything else is logged as an error with its status and the start of the response body. Secrets in that text are masked, as in other output.
- **The log is kept on this machine:** each line is appended to `bulk-log.txt` in personal app data. It is never written to the shared Workspace, because it can name the ids and responses from a real system.
- **No retries:** a failed id is logged and the run moves on. Retrying is left to the user, who can paste the failed ids into a new run.

## Considered Options

- **A script outside the app:** works, but loses the Request's variables, auth and environment, and the user has to keep the token in a file, which [ADR 0004](0004-secrets-in-os-storage.md) is meant to prevent.
- **Use the Collection Runner:** it runs a fixed list of Requests, not one Request for many values, and it doesn't resume.
- **Save progress in the Workspace:** rejected, because progress is personal to one person's run and would cause git churn ([ADR 0003](0003-git-friendly-workspace-files.md)).

## Consequences

- A bulk run sends real traffic, one request per id, so the user must start it themselves, as with Send.
- The ids pasted for a Request are kept in personal state, so a long list isn't lost when the app closes.
- Runs are sequential, so a long list takes as long as its requests combined.
