---
status: proposed
---

# Context menus on sidebar items

Each action for a Request or Folder is a button or a shortcut that someone has to find. Right-clicking an item is how most people try to act on it first, and the sidebar does nothing when they do. We will add a context menu to each Request and Folder with the actions that apply to it, such as rename, duplicate, move, tag, hide and delete. The menu lists the same actions as the buttons, so nothing is available in one place only.

## Decisions within this

- **The menu uses the same commands as the buttons:** each menu entry is a command already used elsewhere, which keeps the behaviour the same and avoids a second copy of each action.
- **Only actions that apply are shown:** a Request has no "move into" for itself, and a hidden Folder shows "show" instead of "hide".
- **Shortcuts are shown beside the entry:** the menu teaches the shortcut from [ADR 0040](0040-keyboard-shortcuts-and-a-command-palette.md) rather than hiding it.
- **Keyboard users can open it too:** the Menu key and Shift+F10 open the same menu, from the focused item.

## Considered Options

- **Buttons only:** what exists, but it hides actions from people who look for them with the mouse.
- **A single action bar for the selected item:** visible without a click, but it takes space in the sidebar, which is already tight after [ADR 0042](0042-denser-layout-and-larger-controls.md).
- **Double-click for the main action:** quick for one action, but it conflicts with rename and with opening a Request.

## Consequences

- Menus must not appear for items where the action would be refused, so each menu is built from the item's state, in the view model rather than in the view.
- A menu that is too long for the window must still open on screen, which Avalonia handles, but this must be checked at the smallest supported size.
- The menu needs to be included in the keyboard and screen reader audit in [ADR 0052](0052-keyboard-and-screen-reader-audit.md).
