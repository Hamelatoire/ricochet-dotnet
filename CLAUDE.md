# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

# Ricochet

A fast, native desktop HTTP client built with C#, Avalonia and Native AOT, plus a `ricochet` CLI for CI. It can take over work exported from Postman.

Use the terms in `GLOSSARY.md`, such as Request, Collection, Workspace and Secret. Check `docs/adr/` before changing how something works: each ADR records why, and one marked `proposed` isn't built.

## Commands

```bash
dotnet run --project src/Ricochet.App
dotnet run --project src/Ricochet.Cli -- run --env Staging --xunit results.xml
dotnet test --project tests/Ricochet.Core.Tests
dotnet test --project tests/Ricochet.Core.Tests -- --filter-class '*VariablesInRequestsTests'
dotnet test --project tests/Ricochet.Core.Tests -- --filter-method '*Bearer*'
dotnet publish src/Ricochet.App -c Release -r win-x64 -o publish/win-x64
```

- `dotnet test` needs `--project`, because `global.json` selects Microsoft.Testing.Platform. Filters go after `--`.
- Run `dotnet publish` on the operating system you're building for, because Native AOT doesn't cross-compile. A normal build won't show AOT problems, so publish after changing anything that serializes or uses reflection.
- If linking fails on Windows with `'vswhere.exe' is not recognized`, add `C:\Program Files (x86)\Microsoft Visual Studio\Installer` to `PATH`.
- No linter or formatter is configured. Tests cover `Ricochet.Core` only, so check App changes in the running app.

## Architecture

`Ricochet.Core` holds all behaviour. `Ricochet.App` (Avalonia, CommunityToolkit.Mvvm, compiled bindings) and `Ricochet.Cli` are thin layers over it, and don't reference each other.

- **Sending:** `VariableResolver` layers the scopes and replaces `{{name}}`, then `RequestSender` sends the result. Its `FindPlaceholders` lists what stayed unresolved. The app warns and still sends, while `CollectionRunner` fails the Request without sending it.
- **Running:** `CollectionRunner` runs Requests in order and checks `Assertions`, and never changes the Workspace. Captured values stay in memory and reset for each Collection. The CLI maps results to exit codes: `0` passed, `1` a Request or check failed, `2` the run couldn't happen.
- **Storage:** `WorkspaceStore` keeps two homes apart. `WorkspaceFolder` holds the shareable files, one JSON file per Request, and watches for outside changes such as a `git pull`. Personal `state.json` and `history.json` stay in app data, or in `RICOCHET_DATA` if set.
- **Secrets:** `WorkspaceSecrets` takes Secrets out before saving and restores them after loading. `SecretVaults` picks the store for each OS: DPAPI, Keychain or Secret Service. Only Windows has been tested. `SecretRules` finds credentials typed in plain text, and hides them in History.
- **Import:** `PostmanImporter` is a one-time conversion into the Core model, and reports whatever it couldn't carry over. Scripts are kept but never run.

## Rules the code depends on

- **JSON uses source generation:** a new type that gets saved must be registered on `WorkspaceJsonContext` in `WorkspaceStore.cs`, or it fails under Native AOT.
- **Saved records use `set`, not `init`:** the generator nulls init-only properties missing from a file, which bypasses their defaults.
- **Keep file diffs small:** optional fields stay null when empty, so they add nothing to a Request's file, as `Assertions` and `Captures` do.
- **Only touch what was understood:** a save must change or delete only files the app read successfully or wrote itself. Files with merge conflicts are reported and left alone.
- **No traffic the user didn't ask for:** [ADR 0005](docs/adr/0005-local-first.md) rules out accounts, cloud storage, telemetry and background network calls.
- **Secrets stay out of the Workspace and the output:** they never go in Workspace files, and the CLI masks them in its output and reports. Captured values are never written to a file.

## Testing the app

Never test against real user data. Set `RICOCHET_DATA` to a temporary folder before launching the app.

## Personal config

- Never commit personal config. This includes `.claude/` and `.vscode/` settings, `launch.json`, `.env` files, `*.user` files, local machine paths, personal email addresses and credentials.
- Stage files by exact path, not with `git add .` or `git add -A`, and check what is staged before committing.
- If a task needs a local config file, keep it uncommitted.

## Pull requests

- `main` only accepts pull requests. `Check win-x64`, `Check linux-x64` and `Check osx-arm64` from `.github/workflows/pr.yml` must pass, and a ruleset on GitHub requires them by name, so renaming that job means updating the ruleset.
- Every merge to `main` publishes the next patch release, including merges that only change docs. To choose the version, push a tag such as `v0.2.0`.
- Every pull request must include screenshots of what it changes, taken from the running app. Show the page before and after when the change alters something that already exists.
- If a pull request changes nothing visible, such as documentation or build files, say so in place of the screenshots.
- Commit screenshots to `docs/screenshots/` on the pull request's branch, and link them from its description using a raw GitHub URL with the commit's hash.
- Take screenshots with throwaway data by setting `RICOCHET_DATA` to a temporary folder, so that they never show real requests, credentials or personal details.
