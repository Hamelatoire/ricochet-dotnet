---
status: accepted (built)
---

# Filter the Collection tree in place

The command palette ([ADR 0040](0040-keyboard-shortcuts-and-a-command-palette.md)) finds a Request by name and opens it, but it shows one list at a time and closes after each choice. Someone looking over a large Collection wants to see every match at once, in its place in the tree, and to narrow the tree as they type. We will add a filter box at the top of the sidebar that hides every Folder and Request that doesn't match, and keeps the parents of each match visible so its place is clear.

## Decisions within this

- **Matches keep their place:** a matching Request is shown with its Folders, so it's clear where it lives, and the Folders don't have to match too.
- **Hidden items stay hidden only while the filter is on:** clearing the box restores the tree as it was, including what was expanded before.
- **Hidden folders stay hidden:** a filter doesn't reveal a Folder hidden under [ADR 0032](0032-hide-a-folder-without-deleting-it.md), so the choice to hide it still holds.
- **Typing is enough:** the match runs as the person types, and Escape clears it.

## Considered Options

- **Rely on the palette only:** one search, but it hides the surrounding tree, which is where a person is working.
- **Filter by tags only:** [ADR 0030](0030-folder-tags-inherited-by-requests-with-runner-filters.md) already filters runs by tag, but tags are not names and not every Request has one.
- **Highlight matches without hiding anything:** easy to read on a small tree, but on a large one the matches are lost in the rest.

## Consequences

- Expanding and collapsing must be remembered separately for the filtered and unfiltered views, or clearing the filter would collapse everything.
- A large tree must filter without a visible pause, which ties to [ADR 0042](0042-denser-layout-and-larger-controls.md)'s row sizes.
- The filter is a view setting, so it stays out of the Workspace and out of the saved layout in [ADR 0043](0043-remember-panel-sizes-and-sidebar-state-per-person.md), unless the person asks to keep it.
