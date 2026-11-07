---
status: proposed
---

# Undo and redo in request edits

A mistaken change to a URL, a header value or a body can only be reversed by retyping it. Ctrl+Z works in a single field, but it stops at the field's edge, so a change made in two fields can't be reversed together, and switching to another Request loses the history. We will keep an undo history for each open Request covering all its fields, so Ctrl+Z and Ctrl+Y reverse the last edit in any field of that Request, whichever field has focus.

## Decisions within this

- **One history per Request:** every field of a Request shares one history, so undoing a header change after a URL change works in order.
- **The history is kept while the Request is open:** switching to another Request and back keeps it, and closing the Request, or the app, drops it.
- **Undo never writes to disk:** the history holds unsaved edits only. Saving is still an explicit step, as it is now.
- **Undo stops at a saved point:** a Request that has been saved shows its last saved state as the start of its history, so undo cannot go back past a save.

## Considered Options

- **Keep field-level undo:** no work, but it fails for the multi-field edits that are most often made by mistake.
- **Save every change and keep a file history:** safe, but it writes to the Workspace on every keystroke, which [ADR 0003](0003-git-friendly-workspace-files.md) rules out for shared files.
- **A global undo stack across all Requests:** simpler to build, but undoing one Request could change another that is open, which surprises people.

## Consequences

- Each open Request holds its own history in memory, so a long session with many Requests open uses more memory. This ties to [ADR 0041](0041-open-several-requests-as-tabs.md).
- Fields that the app sets itself, such as a generated value from a built-in function, must not enter the history, or undo would bring back a stale value.
- The history must be checked against the external-change watch in `WorkspaceFolder`, so that a `git pull` that changes the file doesn't leave an undo that restores old content.
