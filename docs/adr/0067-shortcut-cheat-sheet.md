---
status: proposed
---

# A shortcut cheat sheet

The shortcuts from [ADR 0040](0040-keyboard-shortcuts-and-a-command-palette.md) are only useful if people know they exist. The palette shows a shortcut beside each action it lists, but nothing shows the full set in one place. We will add a cheat sheet opened with F1 or Ctrl+/ that lists every shortcut by area, such as the sidebar, the request view and the response view. The list is built from the same table of commands the shortcuts use, so it can't go out of date.

## Decisions within this

- **Built from the command table:** each shortcut is listed from the command it triggers, so a shortcut that is added or changed appears in the sheet without a separate edit.
- **Grouped by where it works:** shortcuts are grouped by the area they apply in, so a person sees which keys work where they are.
- **Closed with Escape:** the sheet is a small overlay, and Escape closes it, as the other panels do.
- **Read-only:** the sheet shows the shortcuts and doesn't change them. Rebinding is a separate decision, as [ADR 0040](0040-keyboard-shortcuts-and-a-command-palette.md) notes.

## Considered Options

- **A printed page in the documentation:** useful when reading, but nobody reads a page while using the app.
- **Show all shortcuts in the palette:** possible, but the palette is for finding actions, and a full list would crowd the search results.
- **Show a hint on every button:** more visible, but it clutters every control, which [ADR 0042](0042-denser-layout-and-larger-controls.md) is trying to avoid.

## Consequences

- The command table must hold the area and the key for each command, so the table becomes the one source for the palette, the menus and the sheet.
- Each new shortcut must come with an area, or the sheet can't place it, which a test can check.
- The sheet must be reachable by screen reader, following the audit in [ADR 0052](0052-keyboard-and-screen-reader-audit.md).
