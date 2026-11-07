---
status: proposed
---

# Folder-level variables

A value shared by some requests in a collection, but not all, has to live at collection level, so every request sees it. Folder-level variables would let a value apply to one part of a collection only.

A folder can hold variables. They override the collection's values for requests inside that folder, and are overridden by environment values, as in ADR 0002's order.

## Considered Options

- Keep three scopes: rejected, because it forces a collection split just to share a value.

## Consequences

- Variable resolution gets one more layer, so the scope order must be documented and tested.
- Existing files are unchanged, since the field is optional.
