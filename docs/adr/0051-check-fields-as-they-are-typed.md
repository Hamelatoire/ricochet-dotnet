---
status: proposed
---

# Check fields as they are typed

Mistakes in a Request are often found only when it's sent, as a failed request with a message that points away from the cause. A URL without a scheme, a header name with a space in it, or a duplicate header are all visible in the field as soon as they're typed. We will check the URL, header names and JSON bodies while typing, and show a short message beside the field that says what is wrong and how to fix it. Checks warn; they never stop a send, so the app stays out of the way of requests it can't judge.

## Decisions within this

- **Warn, never block:** a warning beside the field is shown, but Send still works, because an unusual header or URL may be valid for the server.
- **Say what to do:** "Add https:// to the start" rather than "invalid URL".
- **Check only what the app can know:** URL scheme and syntax, header names, and whether a JSON body parses. It doesn't check what a server will accept.
- **Unclosed placeholders are left to the send-time check:** an unclosed `{{` is reported when the Request is sent, as [PR #95](https://github.com/Hamelatoire/ricochet-dotnet/pull/95) does. A live warning for it could follow the chips in [ADR 0009](0009-chips-over-plain-text.md), but it is not part of this decision.

## Considered Options

- **Check only at send time:** what exists today, but the problem is found after the wait.
- **Block the send on error:** prevents bad requests, but it blocks valid edge cases and goes against the sender's own rules.
- **Check against the server's description:** more accurate, but it needs the OpenAPI spec and a network call, which [ADR 0005](0005-local-first.md) does not allow in the background.

## Consequences

- Checks run on every keystroke, so they must be cheap, and checking a large JSON body must not slow typing.
- Each message needs to be readable in both themes and announced once by screen readers, not on every keystroke.
- The messages must be the same in the app and the CLI where both report on the same mistake.
