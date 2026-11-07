---
status: accepted (built; verified on Windows only)
---

# Proxy and client certificates for requests

Some networks need requests to go through a proxy, and some APIs require a client certificate. Ricochet has neither setting, so these requests fail or need workarounds.

Proxy and client certificate settings live on the settings page, as personal settings. Certificate files are referenced by path and are never copied into the Workspace.

## Considered Options

- Leave proxies to the operating system: this stays the default.

## Consequences

- A certificate password is a Secret (ADR 0004).
- Settings are personal, so a shared collection does not change another person's proxy.

## What was built

- **Proxy address** on the settings page: an http or https proxy, such as `proxy.example.com:8080`. Empty keeps the operating system's proxy, which is the default. Proxy logins are not supported yet, and an address with a user name or password is refused rather than stored in plain text.
- **Client certificate file**: a .pfx or .p12 file, referenced by path and never copied. Its password goes to secure storage under `personal/clientCertificatePassword`, separate from Workspace Secrets, so saving a Workspace keeps it. The password is never written to `state.json` or the Workspace.
- `RequestSender.Configure` applies both settings and returns the problem when one cannot be used. Requests then fail with that message, and the settings page shows it beside the fields.
- Fixed while building this: the view model read personal settings before the state file was loaded, so every personal setting from [ADR 0007](0007-personal-settings-page.md) was ignored after a restart.

Not built: PEM or key-file certificates, proxy logins, CLI options for these settings, and per-Collection settings.
