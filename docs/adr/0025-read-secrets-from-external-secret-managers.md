---
status: accepted (built in Core: environment variables and commands; not yet used by sending or the CLI)
---

# Read secrets from external secret managers

Teams often keep API credentials in a secret manager such as HashiCorp Vault or a cloud key vault. Ricochet reads secrets only from the operating system's secure storage (ADR 0004), so these must be copied in by hand.

Ricochet may read a Secret from a configured secret manager, but only when the user runs a request or the CLI asks for it, and the secret is never written to the Workspace.

## Considered Options

- No external managers: keeps the local-first rule (ADR 0005) simple.
- Store the manager's credentials in the Workspace: rejected, because that would commit them.

## Consequences

- Each manager needs its own network and sign-in code, so they can be added one at a time.
- CI systems can use this instead of RICOCHET_VAR_ variables.

## What was built

- A Variable's value can be a reference: `secret-ref:env:NAME` reads an environment variable, and `secret-ref:command:LINE` runs a command the user writes and uses its output (final line break removed). The reference holds no secret, so it is saved in the Workspace as written. Only the reference is saved, never the resolved value.
- `ExternalSecretResolver` (in `src/Ricochet.Core/ExternalSecrets.cs`) resolves a reference on demand. It is read-only: nothing is stored, and the vault (`SecretVaults`) is unchanged.
- Commands run through `cmd.exe` on Windows and `/bin/sh` elsewhere, with no input and a 30 second limit. Their error output is never shown, because it may echo a value, and a failed command reports only its exit code.
- A missing or empty source is an error that names the variable, never a value. The caller must mask the resolved value wherever it appears, as the CLI does for stored Secrets.
- Not built yet: sending a Request and the CLI do not resolve references. Wiring them in is a follow-up, because it touches `RequestSender` and the CLI.
- Tests use a fake environment and a fake command runner, so no real secret store is needed. Verified on Windows only.
