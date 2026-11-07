---
status: accepted (built; not yet shown on a Request's own Variables tab)
---

# Folder-level variables

A value shared by some requests in a collection, but not all, has to live at collection level, so every request sees it. Folder-level variables would let a value apply to one part of a collection only.

A folder can hold variables. They override the collection's values for requests inside that folder, and are overridden by environment values, as in ADR 0002's order.

## Considered Options

- Keep three scopes: rejected, because it forces a collection split just to share a value.

## Consequences

- Variable resolution gets one more layer, so the scope order must be documented and tested.
- Existing files are unchanged, since the field is optional.

## Built

- Order, lowest first: Globals, Collection, Folders from outermost to innermost, Environment, captured values, then overrides. `VariableResolver.FolderScope` finds the Folders around a Request, and the sending, bulk and collection-run paths all layer them in that order.
- A Folder's Variables are written as `variables` in its `folder.json`, and left out when empty, so existing files do not change. Secret Variables on a Folder go to the vault like Collection Secrets.
- The Variables page has an "Editing" picker that lists the Collection and each Folder in it.
- Not yet shown on a Request's own Variables tab, which still lists only the Collection's values.
