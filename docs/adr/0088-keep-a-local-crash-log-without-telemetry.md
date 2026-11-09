---
status: accepted
---

# Keep a local crash log, without sending it anywhere

When the app crashes, the only trace is what the person remembers, and a bug report then has no details to work from. A crash report service would send data off the machine, which [ADR 0005](0005-local-first.md) rules out. We will write a local crash log to the personal folder, with the exception, the app version and the operating system, and no request contents. The person can open the folder and attach the log to a bug report themselves, if they choose.

## Decisions within this

- **Written only on this machine:** the log stays in the personal folder, and nothing is uploaded, consistent with [ADR 0005](0005-local-first.md).
- **No request data:** URLs, headers, bodies, Variable values and Secrets are never written, so a crash log can't leak them.
- **Exception details without values:** the exception type and stack trace are written, and any text from the app's own state is left out.
- **The person chooses to share:** the app shows where the log is, and a button opens its folder. Nothing is attached automatically.

## Considered Options

- **A hosted crash service:** the most useful data, but it sends data off the machine, which the project does not do.
- **No log at all:** no risk, but reports can't be investigated.
- **Log everything, including requests:** the most detail, but it writes Secrets and personal data to disk in a plain file.

## Consequences

- A stack trace can contain file paths, which include the person's user name, so the log must say so, and the paths must be trimmed where possible.
- The log must not grow without limit, so older crash logs are removed past a set count.
- Writing the log during a crash must not itself crash, so the writer must be kept minimal and wrapped in its own error handling.
