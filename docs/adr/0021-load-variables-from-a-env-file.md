---
status: proposed
---

# Load variables from a .env file

Developers often keep configuration in a .env file. Ricochet only reads environment variables named RICOCHET_VAR_ (see the README) and the environments inside the app, so values kept in a project's .env file must be entered again.

Ricochet will read an optional .env file in the workspace folder for local runs. Its values are treated as Secrets, so they never go into the shared files, and they are masked in output.

## Considered Options

- Read .env as plain variables: rejected, because it would leak values into shared files.
- Support only environment variables: kept as the default for CI; the .env file is for local use.

## Consequences

- The .env file must be listed in .gitignore, which the workspace .gitignore decision covers (ADR 0031).
