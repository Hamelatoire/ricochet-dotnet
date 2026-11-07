---
status: proposed
---

# Authentication beyond bearer, basic and API key

Ricochet supports bearer, basic and API key auth. Many APIs use Digest, OAuth 1.0, OAuth 2.0, AWS request signing or NTLM. These are reported on import but not sent (ADR 0002).

Ricochet will add auth types one at a time, each in its own change. OAuth 2.0 needs a sign-in in the browser, so it is only started by the user pressing a button.

## Considered Options

- Add all types at once: rejected, because each one carries security decisions.
- Handle OAuth tokens without a browser: rejected, because most providers need one.

## Consequences

- OAuth 2.0 needs a local redirect listener, which is a network decision under ADR 0005 and must be stated to the user.
- Tokens are Secrets (ADR 0004).
