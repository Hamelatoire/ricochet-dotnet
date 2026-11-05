---
status: accepted
---

# Local-first: no accounts, no cloud, no telemetry

Forced sign-in and cloud storage are among the main reasons people look for a Postman alternative, and an API client handles credentials and internal URLs that many companies can't send to a third party. We commit to Ricochet working fully without an account or internet access, and to it sending nothing except the Requests the user chooses to send. This is recorded as a commitment because it rules out features that would otherwise look like obvious additions.

## What this means

- **No accounts:** nothing in the app requires signing in to a Ricochet service.
- **No cloud storage or sync:** sharing happens through files and git ([ADR 0003](0003-git-friendly-workspace-files.md)).
- **No telemetry:** no analytics or automatic crash reports. Someone can choose to attach a crash log to a bug report themselves.
- **No background traffic:** any future network feature, such as checking for updates, must be off until the user turns it on, and must say what it sends.

## Considered Options

- **Optional account with sync:** convenient across devices, but it means running a service with sensitive data, and it tends to grow from optional into expected.
- **Anonymous opt-in telemetry:** would show which features matter, but even opt-in telemetry adds network code, and undermines the simple promise that nothing leaves the machine.

## Consequences

- We won't have usage data, so priorities must come from issues and conversations with users.
- There is no real-time collaboration, shared workspaces or team administration, which rules out some team buyers.
- Syncing between a person's own machines depends on git or their own file sync, and Secrets don't sync at all ([ADR 0004](0004-secrets-in-os-storage.md)).
- This could be tested automatically by running the app's normal flows and failing on any connection the user didn't request.
