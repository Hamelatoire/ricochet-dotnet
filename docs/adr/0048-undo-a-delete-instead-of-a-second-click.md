---
status: proposed
---

# Undo a delete instead of asking for a second click

Deleting a Request or Folder currently takes a second click within a few seconds, because the delete can't be undone. A second click is easy to make by accident, and it doesn't say what will go. We will replace it with an immediate delete that shows a short notice naming what was deleted, with an Undo button for a few seconds. The deleted item is kept in memory until the Undo window ends, so the file on disk is only removed once it can no longer be restored from the notice.

## Decisions within this

- **The notice names the item:** "Deleted 'Create order' — Undo" says exactly what was removed, so the choice is visible without a dialog.
- **Undo restores the item in place:** the Request or Folder returns to its old position, with its contents and its place in any run order.
- **Deleting a Folder deletes its contents together:** Undo brings back the whole Folder, not only the item that was clicked.
- **Files are removed after the window closes:** nothing is removed from disk until the Undo window passes, so a crash in that time leaves the file in place.
- **Hiding is offered as the safer choice:** the notice can point to hiding a Folder ([ADR 0032](0032-hide-a-folder-without-deleting-it.md)) for items that are only unused.

## Considered Options

- **Keep the second click:** no change, but it stays easy to misfire and doesn't say what is lost.
- **A confirmation dialog:** clear, but it's one more interruption for every delete, and people learn to click through it.
- **Move deleted items to a trash folder in the Workspace:** recoverable later, but it leaves deleted files in shared files, which people would then have to clean up.

## Consequences

- Undo is held in memory for a few seconds, so a crash in that window loses nothing on disk but also restores nothing. This is acceptable because the file is still there.
- Deleting a Folder while a Run is using its Requests must wait, or the Run would report missing Requests.
- The notice needs the same banner style as other notices, per [ADR 0039](0039-keep-views-thin-by-moving-ui-logic-out-of-code-behind.md)'s view-model rule.
