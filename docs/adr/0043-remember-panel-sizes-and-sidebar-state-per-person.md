---
status: accepted
---

# Remember panel sizes and sidebar state for each person

The window's split between sidebar, request and response is fixed at start, so each launch needs the same drags to get the layout back. Someone who makes the request panel taller for a long body, or hides a folder, has to do it again next time. We will remember the size of each split and the sidebar's collapsed state, and restore them on the next launch. These are personal, so they follow the same rule as other personal state: kept in `state.json` on the machine, never in the Workspace.

## Decisions within this

- **Only layout is remembered:** the sizes of the splits, whether the sidebar is collapsed, and its width. Nothing about the content of a Request is stored here.
- **Stored with other personal state:** `state.json` in app data, or `RICOCHET_DATA` if set, as [ADR 0003](0003-git-friendly-workspace-files.md) and the Workspace rules set out.
- **A missing or damaged value falls back to the default:** a bad size is ignored and the default is used, so the window always opens usable.
- **Sizes are clamped to the window:** a saved size that no longer fits, for example after a smaller screen is used, is reduced to fit rather than hiding a panel.

## Considered Options

- **Remember nothing:** no state to keep, but the same drags every launch.
- **Store the layout in the Workspace:** shared between a team, but each person's screen is different, so a shared layout would suit nobody and would cause file changes on every drag.
- **Store it in the operating system's registry or preferences:** native on each system, but it's a second place for personal state on Windows and macOS, and it would be the only settings store outside `state.json`.

## Consequences

- `state.json` gets a few more fields. Those are optional and default to null, so an older file opens unchanged.
- Restoring the layout happens before the first frame, which must not cause a visible jump. This is checked by hand because the automation does not measure it.
- Reopening the app with a hidden folder still hides it, since that is a Workspace choice in [ADR 0032](0032-hide-a-folder-without-deleting-it.md), not a layout choice.
