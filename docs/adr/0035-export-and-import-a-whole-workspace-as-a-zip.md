---
status: accepted
---

# Export and import a whole workspace as a ZIP

Moving a workspace to another machine, or sharing it with someone who does not use git, means copying its folder by hand, which is easy to do wrong.

Ricochet may export the shareable workspace folder as one ZIP, and import a ZIP into a chosen folder. Personal files, History and secrets are never included.

## Considered Options

- Copy the folder by hand: stays available, but is easy to get wrong.

## Consequences

- A ZIP is a single file that can be shared, so the secret exclusion must be tested.
- Imports must not overwrite an existing workspace (ADR 0002).

## What was built

`WorkspaceArchive` in `src/Ricochet.Core/WorkspaceArchive.cs` (Core only; no App or CLI screen yet).

- **Export** includes only the files `WorkspaceFolder` reads as shared: `ricochet.json`, `globals.json`, `environments/*.json` and `collections/**/*.json`. `state.json` and `history.json` are excluded by name, and `secrets.dat` and `.env` are never in that layout. `.gitignore` is not included. Every file must pass the import check, or nothing is written.
- **Import** refuses any entry that is absolute, rooted, uses a backslash or drive letter, or has a `.` or `..` part. It also refuses any file outside the workspace layout, any file that is not UTF-8 JSON in the workspace format, and an archive without `ricochet.json`. It writes into a new or empty folder only, never over an existing workspace. It writes the same `.gitignore` a new workspace gets (ADR 0031).
- Tests: `tests/Ricochet.Core.Tests/WorkspaceArchiveTests.cs`.
