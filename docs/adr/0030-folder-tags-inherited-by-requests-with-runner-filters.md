---
status: accepted
---

# Folder tags, inherited by requests, with runner filters

Ricochet's CLI runs every request in a workspace or a chosen folder. There is no way to run only the requests tagged, for example, as smoke tests, so CI runs the full set every time.

A folder can have tags. Requests inherit them, and the runner and CLI can include or exclude requests by tag. Tags are stored in the folder file.

## Considered Options

- Filter by folder only: rejected, because tags can cross folders.

## Consequences

- Folder files gain an optional field, which stays empty when unused.
- The CLI exit codes (see the README) are unchanged.

## What was built

- Folders take tags, saved as `tags` in `folder.json` and left out when empty. Tags are trimmed, de-duplicated ignoring case, and a Request has every tag of the Folders above it (`FolderTags`, `CollectionRunner`).
- `CollectionRunner` takes `IncludeTags` (any match) and `ExcludeTags` (any match, which wins). Selecting nothing is a setup error, so it exits `2`.
- The CLI has `--tag <name>` and `--exclude-tag <name>`, both repeatable.
- The App edits a folder's tags with the `#` button in its row. Its tag filter box shows only tagged folders and requests, and those inside a tagged folder. The App has no run action, so its filter narrows the sidebar, not runs.
