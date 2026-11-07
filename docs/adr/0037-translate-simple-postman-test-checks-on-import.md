---
status: proposed
---

# Translate simple Postman test checks on import

Postman collections imported into Ricochet keep their tests as scripts, which Ricochet does not run (ADR 0002, ADR 0006). Those tests do nothing after import.

On import, Ricochet converts the simple checks it recognises into assertions (ADR 0006). Anything it doesn't recognise stays as a kept script, reported in the import report.

## Considered Options

- Keep scripts only: what happens today, and tests stop working.
- Run the scripts: rejected, because that needs a JavaScript engine (ADR 0006).

## Consequences

- Each recognised pattern needs a test, so the list of supported checks must be published.
- A partial translation can give false confidence, so the report must list what was not converted.
