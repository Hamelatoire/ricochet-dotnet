---
status: proposed
---

# Version the Workspace file format and migrate it

Workspace files are plain JSON, and their shape has changed as features were added, such as Assertions, Captures and folder tags. The files carry no version, so the app can't tell an old file from a new one except by guessing from which fields exist. A future change that renames a field would silently break files that teammates haven't updated. We will add a format version to each Workspace, and a migration step that brings an older file up to the current version when it is read, leaving the file unchanged until the person saves it.

## Decisions within this

- **A version in each Workspace:** a small file in the Workspace records its format version, so the app knows what it is reading.
- **Migrate on read, write on save:** an old file is converted in memory when loaded, and written in the new shape only when the person saves, so opening a file never changes it.
- **Newer files are refused:** a Workspace written by a newer version of the app is opened read-only with a message, rather than being rewritten in an older shape that loses fields.
- **Each migration is a tested step:** every format change ships with a migration and a test that reads a file of the old shape.

## Considered Options

- **Keep guessing from fields:** no work, but each new field makes the guesses harder.
- **Rewrite all files on upgrade:** simple, but it produces a large diff in every file, which [ADR 0003](0003-git-friendly-workspace-files.md) is designed to avoid.
- **A version per file:** precise, but a Workspace is read as a whole, so one version per Workspace is enough.

## Consequences

- The version file is one more file in the Workspace, so it must be treated like the others by the save and watch rules in `WorkspaceFolder`.
- Migrations add code that is rarely run, so each one must be kept until no supported old version remains.
- The file must be registered for JSON source generation, following the rule in [CLAUDE.md](../../CLAUDE.md), so it works under Native AOT.
