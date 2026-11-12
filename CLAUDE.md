# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

# Ricochet

A fast, native desktop HTTP client built with Rust and GPUI, plus a `ricochet` CLI for CI. It can take over work exported from Postman and Insomnia. It was rebuilt from C# ([ADR 0091](docs/adr/0091-rebuild-in-rust-with-gpui.md)), and the C# code is gone from this repository.

Use the terms in `GLOSSARY.md`, such as Request, Collection, Workspace and Secret. Check `docs/adr/` before changing how something works: each ADR records why, and one marked `proposed` isn't built. ADRs written before the rebuild describe the C# code, so read them for the reasons, not the file names.

## Commands

```bash
cargo run --manifest-path rust/ricochet-app/Cargo.toml
cargo run --manifest-path rust/ricochet-cli/Cargo.toml -- run --env Staging --xunit results.xml
cargo test --manifest-path rust/ricochet-core/Cargo.toml
cargo test --manifest-path rust/ricochet-core/Cargo.toml --test variables
cargo test --manifest-path rust/ricochet-core/Cargo.toml bearer
cargo build --release --manifest-path rust/ricochet-app/Cargo.toml
```

- The three crates under `rust/` are not a Cargo workspace, so pass `--manifest-path` or run Cargo inside the crate.
- `ricochet-core` and `ricochet-cli` have tests, and so does `ricochet-app` for the logic behind the window. Check changes to what the window draws in the running app.
- The core tests include the Secret vault tests, which use each OS's real store. `pr.yml` runs them on Linux inside a session bus with an unlocked keyring.
- No linter or formatter is configured.

## Architecture

`ricochet-core` holds all behaviour. `ricochet-app` (GPUI) and `ricochet-cli` are thin layers over it, and don't reference each other.

- **Sending:** `variables.rs` layers the scopes and replaces `{{name}}`, then `send.rs` sends the result. `find_placeholders` lists what stayed unresolved. The app warns and still sends, while `collection_runner.rs` fails the Request without sending it.
- **Running:** `collection_runner.rs` runs Requests in order and checks `assertions.rs`, and never changes the Workspace. Captured values stay in memory and reset for each Collection. The CLI maps results to exit codes: `0` passed, `1` a Request or check failed, `2` the run couldn't happen.
- **Storage:** `store.rs` keeps two homes apart. The Workspace folder holds the shareable files, one JSON file per Request, and the app watches for outside changes such as a `git pull`. Personal `state.json` and `history.json` stay in app data, or in `RICOCHET_DATA` if set (`personal_state.rs`, `history.rs`).
- **Secrets:** `workspace_secrets.rs` takes Secrets out before saving and restores them after loading. `secrets_os.rs` picks the store for each OS: DPAPI, Keychain or Secret Service. Only Windows has been tested. `external_secrets.rs` handles Secrets that live in another store. Credentials typed in plain text are found and hidden in History.
- **Import:** `postman.rs` and `insomnia.rs` are one-time conversions into the core model, and report whatever they couldn't carry over. Scripts are kept but never run.

## Rules the code depends on

- **Saved records use serde defaults:** a field missing from a file gets its default (`#[serde(default)]`), so an older file still loads. Add a default to every new field of a saved type.
- **Keep file diffs small:** optional fields stay out of a Request's file when empty (`skip_serializing_if`), as `assertions` and `captures` do.
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

- `main` only accepts pull requests. `Check win-x64`, `Check linux-x64` and `Check osx-arm64` from `.github/workflows/pr.yml` must pass, and a ruleset on GitHub requires them by name, so renaming that job means updating the ruleset. They run the Rust tests and builds.
- Every merge to `main` publishes the next patch release, including merges that only change docs. To choose the version, push a tag such as `v0.2.0`.
- Every pull request must include screenshots of what it changes, taken from the running app. Show the page before and after when the change alters something that already exists.
- If a pull request changes nothing visible, such as documentation or build files, say so in place of the screenshots.
- Commit screenshots to `docs/screenshots/` on the pull request's branch, and link them from its description using a raw GitHub URL with the commit's hash.
- Take screenshots with throwaway data by setting `RICOCHET_DATA` to a temporary folder, so that they never show real requests, credentials or personal details.
- When a pull request changes the look of the app, retake each screenshot in `README.md` that shows the changed screen, in both themes where the README shows both, and commit the new versions in the same pull request.
- When a pull request removes a screenshot from `README.md`, or any screenshot in `docs/screenshots/` is no longer referenced by `README.md`, `docs/`, or the code, delete it in the same pull request. Older pull request descriptions may still link to deleted images, and that is acceptable.
