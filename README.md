# Ricochet

<img src="rust/ricochet-app/assets/ricochet-256.png" alt="The Ricochet logo" width="96" align="right">

A fast, native desktop HTTP client for Windows, macOS and Linux, with a `ricochet` command for CI. Your work is a folder of small JSON files you can keep in git, and nothing is sent anywhere you didn't ask for.

Ricochet is written in Rust with [GPUI](https://www.gpui.rs). [ADR 0091](docs/adr/0091-rebuild-in-rust-with-gpui.md) records why it was rebuilt from C#.

- Decisions: [docs/adr](docs/adr)
- Vocabulary: [GLOSSARY.md](GLOSSARY.md)
- What Ricochet stores and sends: [docs/PRIVACY.md](docs/PRIVACY.md)

## Features

- Send requests with headers, bodies and authentication, including signed kinds such as Digest, OAuth 1.0 and AWS Signature v4.
- Organise requests into collections and folders, with environments and scoped `{{variables}}`.
- Open several requests as tabs. Press Ctrl+K (Cmd+K on macOS) for a palette of actions and requests.
- Keep secrets in your operating system's secure storage, out of the workspace files.
- Add checks to a request, and save values from a response for later requests.
- Import Postman and Insomnia exports, or build a collection from an OpenAPI 3 description.
- Run a collection once for each id or data row, with **Bulk run**.
- Answer requests from saved examples or an OpenAPI description with the **Mock server**.
- Run a workspace from the command line in CI.

## Install on Windows

Download `ricochet-setup-<version>-win-x64.exe` from the [latest release](https://github.com/Hamelatoire/ricochet-http-client/releases/latest) and run it. The installer puts Ricochet in your own programs folder, so it needs no administrator rights. It can also add a desktop shortcut and put the `ricochet` command on your `PATH`, and both are off until you choose them. Uninstalling removes the program and leaves your workspace and settings alone.

Release builds aren't signed yet, so Windows may warn before running the installer. The zips and tarballs for each platform on the same page need no installer.

## Your data

Your work is stored as a folder of small JSON files, one per request, so it can be shared through git. By default that folder is in your user application data (`%APPDATA%\Ricochet\workspace` on Windows). To keep it in a repository, open or copy the workspace to another folder from the app. If files change while Ricochet is open, for example after a `git pull`, it reloads them. Files it can't read, such as one with merge conflict markers, are reported and left untouched.

History, the active environment and secrets stay in your personal application data and are never written to the shared folder. Set `RICOCHET_DATA` to use a different personal folder. For what Ricochet stores and sends, see the [user data policy](docs/PRIVACY.md).

Values you mark as **Secret**, and values typed into auth fields, are kept out of the workspace and in your system's secure storage instead: DPAPI on Windows, Keychain on macOS, and Secret Service on Linux (which needs `secret-tool` from `libsecret-tools`). Only Windows has been tested. If secure storage isn't available, Secrets last only until you close the app, and Ricochet tells you so. See [ADR 0092](docs/adr/0092-secrets-in-the-rust-port.md).

**Everything else in the workspace folder is plain text, and is shared if you commit it.** That includes other variables and any credentials typed directly into a header or URL, though Ricochet warns you when it spots one. Use a `{{variable}}` marked Secret for these. History hides common credential headers.

## Test your APIs

Add checks to a request, such as "Status equals 200" or "JSON value `items[0].id` exists". They run every time you send it, and the results appear beside the response. Save the request to keep them. See [ADR 0006](docs/adr/0006-declarative-tests-not-scripts.md) for why these are checks rather than scripts.

Paths for JSON values look like `items[0].name` or `$.a['b.c']`, and `items.length` counts an array. Expected values can use `{{variables}}`.

To chain requests, save a value from a response. For example, on a login request, save the JSON value `data.token` as `token`, then use `{{token}}` in later requests in that collection. Saved values stay in memory and are never written to a file.

## Import from Postman

Export your collections (v2.0 or v2.1), environments and globals from Postman as JSON, then choose **Import** and select the files. The results list everything that could not be carried over. Scripts are kept but not run, and OAuth 2 and file uploads are not imported yet. See [ADR 0002](docs/adr/0002-import-postman-exports.md).

## Run in CI

The `ricochet` command runs a workspace's requests and checks. A run only reads the workspace. Build it with:

```bash
cargo build --release --manifest-path rust/ricochet-cli/Cargo.toml
```

Then run it from a workspace folder, or give it the path:

```bash
ricochet run --env Staging --xunit results.xml
```

It exits with `0` if everything passed, `1` if a request or check failed, and `2` if the run couldn't happen. These count as `2` rather than passing, because a green result would be misleading: unreadable files such as ones with merge conflicts, and selections that match nothing.

A request needing a variable with no value fails without being sent. Secrets aren't in the repository, so provide them through environment variables named `RICOCHET_VAR_<name>`, for example `RICOCHET_VAR_token`. For a local run, a `.env` file in the workspace folder works too, with `KEY=VALUE` lines. Environment variables override it, and `--var` overrides both. These values and saved values are masked in the output and the report.

Requests run in order, and saved values carry forward within a collection. Folders can have tags, which their requests inherit: `--tag smoke` runs only requests with that tag, and `--exclude-tag slow` skips them. `--data <file>` runs the collection once per row of a `.csv` or `.json` file. See [ADR 0018](docs/adr/0018-run-a-collection-once-per-row-of-a-csv-or-json-data-file.md).

Other commands, each with its own `--help`:

- `ricochet export <collection> --postman <file>` or `--openapi <file>` writes a Postman v2.1 collection or an OpenAPI 3 document. Secret values are never written. See [ADR 0029](docs/adr/0029-export-a-collection-to-postman-and-openapi.md).
- `ricochet env export` and `ricochet env import` move an environment through a JSON file.
- `ricochet docs <collection> <file.html>` writes a page documenting a collection.
- `ricochet zip export` and `ricochet zip import` archive and restore a workspace.
- `ricochet clone <git-url> <folder>` clones a workspace from git.

On Windows, Smart App Control can block a newly built `ricochet.exe` because it's unsigned. Signing release builds is the lasting fix.

## Build from source

You need a stable Rust toolchain. The three crates under [`rust/`](rust) are not a Cargo workspace, so run Cargo with `--manifest-path`:

- `ricochet-core` holds the behaviour: the data model, variables, sending, checks, the runner, import and export, the mock server and the secret vaults.
- `ricochet-app` is the window.
- `ricochet-cli` is the command line.

Set `RICOCHET_DATA` to a temporary folder first, so you don't try the app on your real data.

```bash
cargo run --manifest-path rust/ricochet-app/Cargo.toml
```

```bash
cargo test --manifest-path rust/ricochet-core/Cargo.toml
```

```bash
cargo test --manifest-path rust/ricochet-cli/Cargo.toml
```

```bash
cargo test --manifest-path rust/ricochet-app/Cargo.toml
```

On Linux, GPUI needs the development packages for Wayland, X11 and Vulkan. The CI workflows in [`.github/workflows`](.github/workflows) list the ones they install.

## Release

Merging a pull request into `main` runs the tests and builds the app and the command line on Windows, Linux and macOS, then attaches them to a GitHub release with the next patch version, such as `v0.1.1`. The Windows build also gets an installer, made from `installer/ricochet.iss` with [Inno Setup](https://jrsoftware.org/isinfo.php). The pull request check builds it too, so a change that breaks it is found before it merges.

To choose the version yourself, such as a new minor version, push a tag instead:

```bash
git tag v0.2.0
```

```bash
git push origin v0.2.0
```

The builds are not code signed, so Windows and macOS may warn about them or block them.
