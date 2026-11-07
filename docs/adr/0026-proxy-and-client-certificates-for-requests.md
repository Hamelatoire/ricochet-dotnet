---
status: proposed
---

# Proxy and client certificates for requests

Some networks need requests to go through a proxy, and some APIs require a client certificate. Ricochet has neither setting, so these requests fail or need workarounds.

Proxy and client certificate settings live on the settings page, as personal settings. Certificate files are referenced by path and are never copied into the Workspace.

## Considered Options

- Leave proxies to the operating system: this stays the default.

## Consequences

- A certificate password is a Secret (ADR 0004).
- Settings are personal, so a shared collection does not change another person's proxy.
