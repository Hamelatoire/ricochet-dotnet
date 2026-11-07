---
status: proposed
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
