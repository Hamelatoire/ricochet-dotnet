---
status: proposed
---

# Audit keyboard and screen reader access against a fixed list

The app has named some controls for screen readers and fixed some keyboard paths, but it has never been checked end to end. [ADR 0040](0040-keyboard-shortcuts-and-a-command-palette.md) adds shortcuts, [ADR 0041](0041-open-several-requests-as-tabs.md) adds tabs, and each one could leave a control that can't be reached by keyboard or read aloud. We will set a checklist of every screen and every control, and check each item with the keyboard alone and with a screen reader, before a release. Fixes go in the same change as the audit.

## Decisions within this

- **One checklist, kept in the repo:** it lives in `docs/`, so each release can be checked against it and changes can be made to it.
- **Keyboard only, for every screen:** every control must be reachable with Tab, shown with a visible focus ring, and usable with Enter or Space.
- **Every control has a name:** each button, field and list item has an accessible name that a screen reader reads, using the `AutomationProperties.Name` already used in the app.
- **Checked in both themes:** focus rings and labels must be visible in light and dark, following [ADR 0013](0013-light-and-dark-theme.md).

## Considered Options

- **Check only new screens:** cheaper, but it leaves the older screens as they are, and they are the ones most used.
- **Rely on an automated accessibility scanner:** useful for missing names, but it can't judge whether the order or the wording makes sense.
- **Hire an outside review:** thorough, but it's a cost and a delay for an app at this stage.

## Consequences

- The audit takes time on each release, so it should be sized in the release plan, not left for the end.
- Some fixes will touch every screen, so they must be split into small changes, as [ADR 0039](0039-keep-views-thin-by-moving-ui-logic-out-of-code-behind.md) suggests.
- Screen reader results differ by operating system, so the audit must say which reader and system it was run on.
