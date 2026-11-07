---
status: accepted
---

# Load variables from a .env file

Developers often keep configuration in a .env file. Ricochet only reads environment variables named RICOCHET_VAR_ (see the README) and the environments inside the app, so values kept in a project's .env file must be entered again.

Ricochet will read an optional .env file in the workspace folder for local runs. Its values are treated as Secrets, so they never go into the shared files, and they are masked in output.

## Considered Options

- Read .env as plain variables: rejected, because it would leak values into shared files.
- Support only environment variables: kept as the default for CI; the .env file is for local use.

## Consequences

- The .env file must be listed in .gitignore, which the workspace .gitignore decision covers (ADR 0031).

## Built

`DotEnv` in Core reads `.env` from the workspace folder. Its values are Secrets, and they are the lowest-priority scope: a Global, Collection, Environment or captured value with the same name wins, and `--var` or `RICOCHET_VAR_` win over all of them. The App reads the file on each send and each bulk run; the CLI reads it on each run. Values are masked in CLI output. Nothing returns them as part of a Workspace, so they are never saved. Malformed lines are skipped and reported. The file is not imported or rewritten.
