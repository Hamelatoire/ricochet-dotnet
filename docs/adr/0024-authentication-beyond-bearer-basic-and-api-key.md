---
status: accepted (partly built: Digest, OAuth 1.0 and AWS Signature v4; OAuth 2.0 and NTLM not yet)
---

# Authentication beyond bearer, basic and API key

Ricochet supports bearer, basic and API key auth. Many APIs use Digest, OAuth 1.0, OAuth 2.0, AWS request signing or NTLM. These are reported on import but not sent (ADR 0002).

Ricochet will add auth types one at a time, each in its own change. OAuth 2.0 needs a sign-in in the browser, so it is only started by the user pressing a button.

## Built so far

- **Digest:** sends once, reads the server's challenge, and sends again with the answer. MD5 or SHA-256, qop auth or none. Other algorithms or qop values are refused with a message.
- **OAuth 1.0:** HMAC-SHA1 signing. The consumer secret and token secret are Secrets.
- **AWS Signature v4:** signs host and x-amz-date (and the session token when set). The secret access key and session token are Secrets. S3-style payload signing (`x-amz-content-sha256`) is not included.
- Not yet built: OAuth 2.0 (needs the browser sign-in and redirect listener from the Consequences below) and NTLM. Postman import still reports these kinds as unsupported.

## Considered Options

- Add all types at once: rejected, because each one carries security decisions.
- Handle OAuth tokens without a browser: rejected, because most providers need one.

## Consequences

- OAuth 2.0 needs a local redirect listener, which is a network decision under ADR 0005 and must be stated to the user.
- Tokens are Secrets (ADR 0004).
