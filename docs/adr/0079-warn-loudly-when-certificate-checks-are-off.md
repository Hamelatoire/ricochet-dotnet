---
status: proposed
---

# Warn loudly and per run when certificate checks are off

The CLI has an `--insecure` option that turns off TLS certificate checks, and it prints a single warning line before the run. A warning on one line of a long CI log is easy to miss, and a run that skips certificate checks reports green just like a verified one. The app can also reach the same state through its settings. We will make the check-off state visible in every place it can apply: the CLI prints it at the start and in the summary, the app shows it on each request, and the xUnit report records it, so a passing result can't hide that verification was skipped.

## Decisions within this

- **Stated in the summary:** the run summary says "certificate checks off" when they were off, so the final line a CI log shows carries the fact.
- **Recorded in the report:** the xUnit file includes a property that says checks were off, so a report viewer can flag the run.
- **Shown on each request in the app:** while checks are off, the request view shows an icon and a tooltip, and History records the state for each send.
- **Not a saved setting in the Workspace:** the choice is per run in the CLI, and personal in the app, so a shared file can't turn checks off for everyone who opens it.

## Considered Options

- **Keep the one-line warning:** what exists, and easy to miss in a log.
- **Refuse to run with checks off:** safest, but a local test server with a self-signed certificate is a real use, which [ADR 0007](0007-personal-settings-page.md) and the CLI help both recognise.
- **Require a separate confirmation in CI:** effective for people, but CI has no one to confirm, so it would block the run instead of making its state visible.

## Consequences

- The report format gains a property, so tools that read the xUnit file must ignore unknown properties, which standard readers do.
- A run with checks off must not be presented as a clean pass in any summary, so the wording in the summary needs review.
- The app must not store the off state in the Workspace, consistent with [ADR 0003](0003-git-friendly-workspace-files.md).
