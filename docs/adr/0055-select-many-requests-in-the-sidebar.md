---
status: proposed
---

# Select several Requests in the sidebar for bulk actions

Moving, tagging, hiding or deleting Requests happens one at a time, so reorganising a Collection of forty Requests takes forty clicks. We will let people select several Requests or Folders in the sidebar with Ctrl-click and Shift-click, then apply one action to all of them. Actions that already exist for one item, such as move, tag or hide, apply to the whole selection, and a selection that is partly hidden or partly read-only says so before it changes anything.

## Decisions within this

- **Ctrl-click and Shift-click work as in a file manager:** people already know how these behave, so the sidebar follows them.
- **One action, one undo:** applying an action to a selection is a single step, so it can be reversed with the undo from [ADR 0048](0048-undo-a-delete-instead-of-a-second-click.md).
- **Mixed selections are reported:** if some items can't take an action, the notice names how many were skipped and why, rather than doing part of the job silently.
- **Selection is a view state:** it is not saved to the Workspace or to the layout in [ADR 0043](0043-remember-panel-sizes-and-sidebar-state-per-person.md).

## Considered Options

- **Keep single-item actions only:** simplest, but reorganising a large Collection stays slow.
- **Select with checkboxes on every row:** clear, but it adds clutter to every row, which works against [ADR 0042](0042-denser-layout-and-larger-controls.md).
- **Bulk actions through a separate dialog:** more room to explain, but it moves the work away from the tree where the items are.

## Consequences

- Each action must accept a list of items, not one, which is a change to the view models described in [ADR 0039](0039-keep-views-thin-by-moving-ui-logic-out-of-code-behind.md).
- Moving a selection that spans Folders must keep the order of the items, or the Collection's run order would change unexpectedly.
- The selection must be cleared when the Workspace changes outside the app, as the external-change watch in `WorkspaceFolder` reports.
