---
status: accepted (built)
---

# Generate a .gitignore for new workspaces

A workspace is a folder that can be kept in git (ADR 0003), but nothing stops a personal or secret-holding file from being added by mistake.

When a workspace is created in a folder, Ricochet adds a .gitignore that excludes personal and secret-holding files. If one exists, Ricochet does not overwrite it, and it tells the user what to add.

## Considered Options

- No file: relies on the user remembering the rules.

## Consequences

- The file lives in the shared folder, so it is itself shared, which is the intended effect.
- It must be kept in step with what the app writes (see CLAUDE.md, Personal config).
