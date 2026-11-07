---
status: proposed
---

# Folder tags, inherited by requests, with runner filters

Ricochet's CLI runs every request in a workspace or a chosen folder. There is no way to run only the requests tagged, for example, as smoke tests, so CI runs the full set every time.

A folder can have tags. Requests inherit them, and the runner and CLI can include or exclude requests by tag. Tags are stored in the folder file.

## Considered Options

- Filter by folder only: rejected, because tags can cross folders.

## Consequences

- Folder files gain an optional field, which stays empty when unused.
- The CLI exit codes (see the README) are unchanged.
