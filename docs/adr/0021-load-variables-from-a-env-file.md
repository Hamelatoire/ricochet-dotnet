---
status: accepted
---

# Load variables from a .env file

Developers often keep configuration in a .env file. Ricochet only reads environment variables named RICOCHET_VAR_ (see the README) and the environments inside the app, so values kept in a project's .env file must be entered again.

Ricochet will read an optional .env file in the workspace folder for local runs. The CLI and the app both read it. Its values are treated as Secrets, so they never go into the shared files, and they are masked in output.

## Considered Options

- Read .env as plain variables: rejected, because it would leak values into shared files.
- Support only environment variables: kept as the default for CI; the .env file is for local use.

## Consequences

- The .env file must be listed in .gitignore, which the workspace .gitignore decision covers (ADR 0031).
- Priority, lowest first: .env, then RICOCHET_VAR_ environment variables, then --var. A value from .env is masked in output like any other override.

## Built

The CLI was built first (PR #39). The app now reads the file on each send and each bulk run, and its values are overrides too, so they win over Globals, Collection, Environment and captured values. A skipped line is reported in the app's notice or the bulk log. Nothing returns .env values as part of a Workspace, so they are never saved.

The app's URL hover and chip tips show a .env value as "Secret, hidden", and a saved response example masks it, as they do for any Secret. The values are Secrets because `DotEnv.Parse` marks each non-empty value as one; an empty value stays plain, so it still counts as set. The app does not read RICOCHET_VAR_ environment variables yet, so those values still need the CLI.
