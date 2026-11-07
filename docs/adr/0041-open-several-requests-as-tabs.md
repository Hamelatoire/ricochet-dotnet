---
status: accepted
---

# Open several Requests as tabs

Today one Request is open at a time. Switching to another Request in the sidebar replaces the editor, so comparing two responses or copying a value from one Request into another means remembering it or sending again. We will let several Requests stay open as tabs across the top of the request view. Each tab keeps its own unsent edits, so switching never loses work, and closing a tab with unsent edits asks first.

## Decisions within this

- **Tabs are a view over the Workspace, not a copy of it:** a tab points at a Request in the Collection. Renaming or moving a Request updates its tab, and a Request can be open once only.
- **Unsent edits stay with the tab:** a tab keeps what has been typed but not saved, until the user saves or discards it. Nothing is written to a file just because a tab was opened.
- **Closing asks only when it would lose work:** a tab with no unsent edits closes at once. A tab with edits asks to save or discard.
- **Open tabs are personal:** which tabs are open, and which is active, is saved in `state.json` with the other personal settings, not in the Workspace, as [ADR 0007](0007-personal-settings-page.md) and [ADR 0003](0003-git-friendly-workspace-files.md) keep personal state apart from shared files.
- **A limit keeps the bar usable:** the number of tabs is not capped in code, but the tab bar scrolls and shows a count when there are more tabs than fit.

## Considered Options

- **Keep one Request open, as now:** no new state to track, but comparing or copying between Requests stays slow.
- **Open each Request in a new window:** easy to build, but each window has its own settings and a different way to switch, and it does not fit the current layout.
- **Keep the last few Requests in a history list without tabs:** quick to reach, but it hides the work instead of showing it, and unsent edits still have no home.

## Consequences

- Each open tab holds an editor for its Request, so memory grows with the number of tabs. Since the app is meant to be fast, its start-up and switching time must be measured with many open tabs.
- The unsent-edit state must be kept apart from the saved Request so that a `git pull` that changes a file is detected before a tab saves over it. This connects to the outside-change watch in `WorkspaceFolder`.
- [ADR 0019](0019-prompt-for-a-variable-when-a-request-is-sent.md) and [ADR 0008](0008-built-in-functions-for-generated-values.md) assume one Request at a time in some places, which must be checked when tabs land.
