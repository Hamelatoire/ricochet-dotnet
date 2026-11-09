---
status: proposed
---

# Keep Secrets in the Rust port

[ADR 0004](0004-secrets-in-os-storage.md) keeps Secret values out of Workspace files and in each OS's secure storage. The C# code does this in several places: [SecretVaults.cs](../../src/Ricochet.Core/SecretVaults.cs) holds all Secrets as one protected item, [WorkspaceSecrets.cs](../../src/Ricochet.Core/WorkspaceSecrets.cs) splits Secrets out of a Workspace before saving and merges them back after loading, and [SecretRules.cs](../../src/Ricochet.Core/SecretRules.cs) recognises literal credentials. [PersonalSecrets.cs](../../src/Ricochet.Core/PersonalSecrets.cs) keeps personal Secrets such as the client certificate password ([ADR 0026](0026-proxy-and-client-certificates-for-requests.md)), and [ExternalSecrets.cs](../../src/Ricochet.Core/ExternalSecrets.cs) resolves references to external sources ([ADR 0025](0025-read-secrets-from-external-secret-managers.md)).

The Rust port ([ADR 0091](0091-rebuild-in-rust-with-gpui.md)) has none of this yet. Its models carry the Secret fields, but nothing stores or loads them, and `rust/ricochet-core/src/variables.rs` lists external references as not ported. This ADR proposes how the port handles Secrets. It does not build anything.

Only Windows has been tested for the C# vault. The port inherits that gap, so macOS and Linux have to be tested before cutover.

## Decisions within this

- **One protected item, as in C#.** All Secrets go into one item per OS, serialised as a JSON object. Keys keep the C# scheme: `variable/{scope}/{name}`, `auth/{requestId}/{field}`, `globals/...` through `workspace/{id}/...`, and `personal/...`. Workspace files, `state.json` and `history.json` keep the rules they have today.
- **Same stores as C#, so both apps share one vault.** While the C# and Rust apps run side by side, a Secret saved by one must be readable by the other. The Rust port therefore uses the same Windows file (a DPAPI blob at the same path, current-user scope, no extra entropy), the same macOS Keychain generic password (service `Ricochet`, account `secrets`) and the same Secret Service item (attributes `service=ricochet`, `account=secrets`).
- **One trait per OS.** A `SecretVault` trait with `load` and `save` mirrors `ISecretVault`. Each implementation is chosen at startup by target OS. On an unknown OS, Secrets live in memory for the session.
- **Never overwrite what could not be read.** Carried over from C#: an unreadable Windows file is moved aside to `<name>.unreadable-<timestamp>` and reported; on macOS and Linux the error stops the save. Callers must not write over a vault that failed to load.
- **Reload before every save.** The item is replaced whole on save, so two running apps could delete each other's Secrets. Each save reloads the item and merges only the keys this app changed (see Risks).
- **A redacting type for Secret values.** Secret values are held in a wrapper type whose `Debug` and `Display` print a placeholder, which zeroes its memory on drop and which does not implement `Serialize`. The `secrecy` crate is one option; a small in-house type is the other. Plain `String` is not used for Secret values.
- **Masking from one list.** [ADR 0076](0076-mask-secrets-in-every-output-and-log.md) applies unchanged. The Rust port builds one list of Secret values per run or export and every writer masks against it: curl text, CLI output, the bulk run log, exports and documentation. `SecretRules` moves over as is, including the sensitive header list, the sensitive query parameter pattern and the `[hidden]` value in History.
- **External references ported as they are.** `secret-ref:env:NAME` reads an environment variable through `std::env::var`. `secret-ref:command:LINE` runs the command with stdin closed, stderr drained and never shown, and a 30 second limit. On Windows the command goes through `cmd.exe /d /s /c` with `raw_arg`, so quoting matches the C# code. Elsewhere it goes through `/bin/sh -c`.
- **Resolve at send time only.** As in [ADR 0025](0025-read-secrets-from-external-secret-managers.md), the resolved value goes into the outgoing request and nowhere else. Requests, saved files and History keep the reference as written.
- **Tests use doubles.** The vault and the command runner are traits, with in-memory fakes for unit tests. A golden test reads a vault fixture written by C# and checks the Rust port's output against C#'s, in both directions.

## How the Rust port reaches each store

| OS | C# today | Rust port (recommended) |
| --- | --- | --- |
| Windows | `ProtectedData` (DPAPI) over a file | `windows` crate, `CryptProtectData` and `CryptUnprotectData`, same file |
| macOS | Legacy `SecKeychain*` generic password, via `LibraryImport` | `security-framework` crate, generic password functions, same service and account |
| Linux | `secret-tool` child process, values over stdin and stdout | `secret-service` crate over D-Bus, same attributes |

## Considered Options

- **A. One native binding per OS, same stores as C# (recommended).** Use the three crates in the table. The Rust app reads the existing vault on each OS, so there is no migration and both apps see the same Secrets. The costs are three platform dependencies, platform-specific code that needs its own tests, and a D-Bus client on Linux.
- **B. One cross-platform crate (`keyring`) for all three.** One API and little platform code. But on Windows it uses Credential Manager, not the DPAPI file, so the Rust app cannot see the C# vault and a migration is needed. Credential Manager also caps a credential's blob at 2,560 bytes, and all Secrets sit in one item, so a large Workspace would fail to save.
- **C. Shell out to OS tools everywhere.** Keep `secret-tool` on Linux, as C# does, and use the `security` command on macOS and PowerShell on Windows. This keeps C# behaviour exactly, but `security add-generic-password -w <value>` puts the value in the process argument list, where other local users can read it with `ps`. It also depends on PowerShell being present on Windows. Rejected for macOS. Acceptable on Linux only as a fallback.

## Consequences

- Each OS needs its own code and tests. Before cutover, CI must run the vault tests on macOS and Linux, not only Windows. On Linux that means a session with a D-Bus bus and a keyring daemon, or an explicit marker that skips the test and a manual check recorded in the PR.
- Keys and file layout are shared with C#. A key the Rust port adds must follow the C# scheme, because each C# save rewrites the whole item.
- The Windows file is written to a temporary file and then moved into place, as C# does, so a crash cannot leave a half-written vault.
- Secrets are still lost when the OS profile or keychain is reset, as [ADR 0004](0004-secrets-in-os-storage.md) already accepts.
- The Linux crate adds a D-Bus dependency tree to the build, which should be checked against the size and startup targets in [ADR 0080](0080-benchmark-startup-and-send-overhead-in-ci.md).

## Risks

1. **Two apps overwrite one item.** While both apps run, one may delete a Secret the other has just saved. Reload-and-merge narrows the window, but a race remains and is not eliminated. The owner should decide whether this is acceptable until cutover.
2. **Format drift.** If the Rust port writes anything C# cannot read, C# moves the Windows file aside as unreadable. The golden tests in both directions are the guard against this.
3. **Command references come from Workspace files.** `secret-ref:command:` runs a command line that may have arrived with a `git pull`. Any teammate's commit can add a command that runs on the next send. [ADR 0025](0025-read-secrets-from-external-secret-managers.md) accepted this for the C# app, and the port inherits it. The owner should confirm it still wants that.
4. **Killing the command.** `std::process::Child::kill` ends only the direct child, so `cmd.exe` can leave the real command running after a timeout. The Windows path needs a Job Object or `taskkill /T /F`. The Rust standard library has no wait-with-timeout, so one has to be built.
5. **Accidental printing.** A derived `Debug`, a `tracing` field, or a panic message can print a value. The redacting type and a review of every `derive` and log call guard against this.
6. **History leaks.** The sensitive header and query lists in `SecretRules` must be ported in full. A header missing from the list would write a literal credential to `history.json` in plain text.
7. **Untested platform paths.** The macOS and Linux code will be the first time these paths run in this codebase, so bugs there are likely to be found by users. Testing is a precondition for cutover, not a follow-up.

## Open questions for the owner

- Should the Rust port share the C# vault (option A), or use its own namespace and migrate once at cutover?
- Is the Linux D-Bus dependency acceptable, or should Linux shell out to `secret-tool` as a fallback?
