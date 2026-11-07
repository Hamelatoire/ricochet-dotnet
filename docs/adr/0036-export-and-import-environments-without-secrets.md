---
status: proposed
---

# Export and import environments, without secrets

Environments cannot be moved to another machine or handed to a teammate. The only copies are inside the workspace files and personal data, and secrets must stay out of anything shared.

Environments can be exported to a JSON file with their secret variables left out. Import reads Ricochet or Postman environment files, and handles name clashes as Replace, Copy or Skip, with an apply-to-all option.

## Considered Options

- Export secrets with a password: rejected for now, because it adds a key-handling decision.

## Consequences

- Postman names and values are converted on import (names limited to letters, digits, dash, underscore and dot; values become strings). These rules must be documented.
- Secrets are never exported (ADR 0004).
