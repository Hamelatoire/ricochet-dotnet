---
status: accepted (built in Core and the CLI; no app UI yet)
---

# Clone a Git repository as a workspace

Ricochet opens a folder that is already on disk, so a teammate must clone a shared workspace with git before Ricochet can use it. That step is outside the app.

Ricochet may clone a repository the user names into a folder they choose, then open it as a workspace. The app starts the clone only when the user asks, and it never pushes.

## Considered Options

- Rely on the user's own git: keeps the app simple, and is what ADR 0011 already does for status.

## Consequences

- This is network traffic the user asks for, under ADR 0005.
- Authentication to the remote uses the user's existing git setup, so Ricochet stores no credentials for it.

## Built

`GitCloner` in `Ricochet.Core` (`src/Ricochet.Core/GitCloner.cs`) runs the installed git, found the same way as ADR 0011's status reader, and does only `git clone`:

- The target folder must exist and be empty. A missing, non-empty or file path is refused before git runs.
- The address can't start with a dash, must not carry a password, and may use only the `file`, `http`, `https`, `ssh` and `git` transports. `GIT_ALLOW_PROTOCOL` enforces the same list inside git, which blocks `ext::`.
- Git runs with `GIT_TERMINAL_PROMPT=0`, so it never prompts for a password.
- After a successful clone, the result is checked with `WorkspaceFolder.LooksLikeWorkspace`. A non-workspace result is reported and left on disk.
- Nothing is committed, pushed or stored. The app UI for this is not built yet.

Tests in `tests/Ricochet.Core.Tests/GitClonerTests.cs` use local bare repositories in a temp folder.
