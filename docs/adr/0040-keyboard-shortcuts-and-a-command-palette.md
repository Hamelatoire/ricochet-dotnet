---
status: accepted
---

# Keyboard shortcuts and a command palette

Most actions are reached by clicking through the sidebar, the toolbar or a button in the request view. Someone who sends many requests a day wants to send, switch environment or jump to a request without leaving the keyboard. We will add a fixed set of keyboard shortcuts for the common actions, and a command palette that lists every action and every Request by name, opened with a single shortcut. Shortcuts are personal preferences, so they stay in the app and never enter the Workspace.

## Decisions within this

- **A small fixed set first:** send the Request, save, new Request, switch Environment, find a Request, and focus the URL. Each shortcut appears next to its action in the menu.
- **One palette for everything:** the palette lists actions and Requests in one filtered list. It replaces the need for a shortcut per Request.
- **Shortcuts follow the operating system's conventions:** Ctrl on Windows and Linux, Cmd on macOS, so the app doesn't teach a new set of keys.
- **Shortcuts don't fight text editing:** while a field has focus, keys that type text keep their meaning. Only the modified shortcuts are global.
- **Rebinding waits:** the defaults are fixed in this change. A rebinding screen is a later decision, and would be a settings page change under [ADR 0007](0007-personal-settings-page.md).

## Considered Options

- **Shortcuts only, no palette:** simple and fast, but finding a Request still means the mouse or a long list of shortcuts.
- **A palette only, no shortcuts:** one place to learn, but the common actions would take several keystrokes each.
- **Let each shortcut be rebound now:** flexible, but it adds a settings screen and conflict handling before anyone has asked for it.

## Consequences

- Actions must be commands on view models before they can be listed in the palette, which depends on [ADR 0039](0039-keep-views-thin-by-moving-ui-logic-out-of-code-behind.md).
- The existing URL key handling in `MainWindow.axaml.cs` must keep working, so the new shortcuts are added alongside it and not on top of it.
- Shortcuts can clash with the screen reader or with the operating system. Each default is checked on Windows before it ships, and macOS is checked before a release that includes it.
- The palette's list is only as good as the names of the actions, so each command needs a short, plain name.
