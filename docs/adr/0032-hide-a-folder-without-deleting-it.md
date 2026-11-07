---
status: proposed
---

# Hide a folder without deleting it

A folder that is not needed for now can only be deleted, which is destructive. Users need to hide it from the sidebar and from runs, and bring it back later.

A folder can be hidden. It stays on disk, is left out of the sidebar and runs, and is listed in the collection file so the choice is shared. Showing it again is one action.

## Considered Options

- Delete folders instead: rejected for hiding, because deletion is destructive.

## Consequences

- A hidden folder's requests are not run, so the CLI must report that it skipped them, and exit with code 2 if that leaves nothing to run.
