---
status: proposed
---

# Generate a request as code

Users want to share a request with a colleague or run it in a terminal, but today they copy the URL, headers and body by hand, and the copy often misses the auth.

A request can be shown as a curl command, and later as other languages. Variables are resolved before generation, and secrets are shown masked unless the user chooses to reveal them, as in ADR 0016.

## Considered Options

- Only curl: first step; other languages can follow in later ADRs.
- Generate from the unresolved request: rejected, because the output would not run.

## Consequences

- Generated code can contain secrets, so the masking rule must apply to the copy as well.
- Output must be tested against the sender's header and body handling.
