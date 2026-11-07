---
status: proposed
---

# Reopen where you left off

Each time the app starts, the person chooses the Collection and Environment again and finds the Request they were working on again. For someone who works on the same API all day, that is the first few clicks of every session. We will remember, for each person, the Collection, the Environment and the open Requests as they were at the last close, and restore them on the next launch. What is remembered is a place to start from, not the content of any Request.

## Decisions within this

- **Remembered per person:** the last Collection, Environment and open tabs are stored in `state.json`, not the Workspace, in line with [ADR 0003](0003-git-friendly-workspace-files.md).
- **Only references are stored:** the file stores which Requests were open, by their file name, never their contents, so no unsaved edit is saved here.
- **A missing item is skipped:** a Request that was deleted or renamed since the last launch is left out, and the app opens without it.
- **Can be turned off:** a setting on the settings page from [ADR 0007](0007-personal-settings-page.md) starts the app empty.

## Considered Options

- **Always start empty:** predictable, but it costs the same clicks every launch.
- **Restore everything, including unsaved edits:** convenient, but unsaved edits would then be kept outside the Workspace, which [ADR 0041](0041-open-several-requests-as-tabs.md) keeps apart.
- **Open the last Request only:** simpler, but it loses the other tabs a person had open.

## Consequences

- Restoring must handle a Workspace that changed outside the app, using the external-change watch in `WorkspaceFolder`.
- The stored list must not grow without limit, so the number of remembered tabs needs a cap.
- The layout that [ADR 0043](0043-remember-panel-sizes-and-sidebar-state-per-person.md) remembers is stored alongside, so both are restored at the same time.
