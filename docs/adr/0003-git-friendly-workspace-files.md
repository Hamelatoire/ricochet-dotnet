---
status: accepted (built)
---

# Store shareable work as one JSON file per Request

Without an account or cloud service ([ADR 0005](0005-local-first.md)), git is how people will share and review API work, and a single file containing everything produces unreadable diffs and constant merge conflicts. We will store a Workspace as a folder the user can place in a repository, with one JSON file per Request, and keep personal state out of that folder. This replaces the single `workspace.json` used today.

```text
<workspace>/                  shareable, can live in git
  ricochet.json               identifies the Workspace
  collections/<name>-<id>/
    collection.json           name, Variables, order of children, scripts
    <folder>-<id>/
      folder.json
      <request>-<id>.json
  environments/<name>-<id>.json
  globals.json

<app data>/Ricochet/          personal, never shared
  state.json                  workspace location, active Environment
  history.json
  secrets.dat                 Secrets of every Workspace, by its id (Windows)
```

## Decisions within this

- **JSON, not YAML or a custom format:** System.Text.Json source generation already works with Native AOT, and it adds no dependency or new syntax to learn.
- **The id stays in the file name:** two Requests can share a name, and renaming only the name portion keeps the id stable.
- **Order lives in the parent file:** file systems sort alphabetically, but imported Collections have a meaningful order. Collections are ordered by the list in `ricochet.json`, and items by their Collection or Folder file.
- **Default location is app data:** people who don't use git never need to think about folders.
- **Only touch what we understood:** a save changes or deletes only files the app read successfully or wrote itself. A file with merge conflict markers, or anything unrecognised, is reported and left exactly as it is.
- **A file keeps its name until the item is renamed in the app:** renaming a file by hand doesn't cause churn in git.
- **Copying into a folder needs confirming:** choosing a folder without a Workspace adds files to it, so a mistaken choice must not do that silently. Each copy gets its own id.

## Considered Options

- **Keep one JSON file:** simplest and already built, but poor for git, and one bad write or merge can affect everything.
- **SQLite:** fast and robust, but its binary file can't be diffed or merged, which defeats the purpose.

## Consequences

- Each save writes only what changed, and damage to one file affects only that item.
- Startup must read many small files, so a large Collection needs measuring against the speed goal, and may need loading on demand.
- Files can change underneath the app after a `git pull`, so the app must detect that and reload. Unsaved edits to the open Request are kept, with a warning, and saving replaces the version on disk. Edits to Variables are saved before the reload is read only when something else triggered a save, so a pull can replace unsaved Variable edits.
- A merge can leave invalid JSON or an order list naming missing files, so loading must report these rather than discard them.
- Existing `workspace.json` files need one more migration.
- Secrets must not be written to this folder, which depends on [ADR 0004](0004-secrets-in-os-storage.md).
