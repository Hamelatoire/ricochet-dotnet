---
status: proposed
---

# Choose which collections to import from a Postman data dump

A Postman data dump holds many collections and environments. Ricochet imports the whole file, so unwanted collections come in and must be deleted by hand.

Ricochet will list the collections and environments inside a Postman data dump, selected by default, and import only the ones the user keeps selected.

## Considered Options

- Import the whole dump: today's behaviour, which forces unwanted collections in.

## Consequences

- The dump is a ZIP that Ricochet must read, so the reader must refuse anything that isn't a valid export.
- Duplicate names follow the rules in ADR 0036.
