---
status: proposed
---

# Mock servers built from examples or an OpenAPI description

Developers often need an API that is not built yet, or one they cannot reach. Ricochet can only send requests to a real server, so there is no way to answer them locally.

Ricochet may run a local mock server that answers requests from saved examples (ADR 0023) or an imported OpenAPI description (ADR 0014). It listens only on localhost, and only while the user starts it.

## Considered Options

- Use a separate tool: keeps Ricochet small, but splits the work between two apps.
- Listen on all interfaces: rejected, because mocks would be reachable from the network.

## Consequences

- This is a large feature, so it needs its own design before any code.
- Opening a listening port is a security decision and needs a clear warning.
