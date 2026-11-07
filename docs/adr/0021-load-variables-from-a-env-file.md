---
status: accepted (built for the CLI; the app does not read .env yet)
---

# Load variables from a .env file

Developers often keep configuration in a .env file. Ricochet only reads environment variables named RICOCHET_VAR_ (see the README) and the environments inside the app, so values kept in a project's .env file must be entered again.

Ricochet will read an optional .env file in the workspace folder for local runs. The CLI reads it; the app does not yet. Its values are treated as Secrets, so they never go into the shared files, and they are masked in output.

## Considered Options

- Read .env as plain variables: rejected, because it would leak values into shared files.
- Support only environment variables: kept as the default for CI; the .env file is for local use.

## Consequences

- The .env file must be listed in .gitignore, which the workspace .gitignore decision covers (ADR 0031).
- Priority, lowest first: .env, then RICOCHET_VAR_ environment variables, then --var. A value from .env is masked in output like any other override.
