---
status: accepted (built in Core and the CLI; no app entry point yet)
---

# Export and import environments, without secrets

Environments cannot be moved to another machine or handed to a teammate. The only copies are inside the workspace files and personal data, and secrets must stay out of anything shared.

Environments can be exported to a JSON file with their secret variables left out. Import reads Ricochet or Postman environment files, and handles name clashes as Replace, Copy or Skip, with an apply-to-all option.

## Considered Options

- Export secrets with a password: rejected for now, because it adds a key-handling decision.

## Consequences

- Postman names and values are converted on import (names limited to letters, digits, dash, underscore and dot; values become strings). These rules must be documented.
- Secrets are never exported (ADR 0004).

## What was built

- `EnvironmentTransfer.Export` writes one Environment as a `ricochet-environment` version 1 file. Secret variables keep their name, enabled flag and `IsSecret`, but have no `value`.
- `EnvironmentTransfer.Import` reads either that format or a Postman environment export. Names that don't fit the rule above have each invalid character replaced with `_`, and the change is reported under "Variable names changed to fit". Secrets without a value are reported as needing to be set again. Collections and globals are refused.
- `EnvironmentTransfer.Place` applies Replace, Copy or Skip to one imported Environment. Replace keeps the existing id, so this machine's Secret values still apply, and a Secret the file leaves blank keeps its local value. Copy numbers the name, for example "Staging (2)". Skip changes nothing.
- Imported Secret values go to secure storage through the existing `WorkspaceSecrets.Split`, so they stay out of workspace files.
- Not built yet: the app's import and export dialogs, the apply-to-all option, and any CLI command. Each Environment is handled one at a time through Core.
