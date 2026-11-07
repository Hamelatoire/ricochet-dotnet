---
status: proposed
---

# Clone a Git repository as a workspace

Ricochet opens a folder that is already on disk, so a teammate must clone a shared workspace with git before Ricochet can use it. That step is outside the app.

Ricochet may clone a repository the user names into a folder they choose, then open it as a workspace. The app starts the clone only when the user asks, and it never pushes.

## Considered Options

- Rely on the user's own git: keeps the app simple, and is what ADR 0011 already does for status.

## Consequences

- This is network traffic the user asks for, under ADR 0005.
- Authentication to the remote uses the user's existing git setup, so Ricochet stores no credentials for it.
