---
status: proposed
---

# Retry idempotent requests with backoff in runs

A Collection run that calls a real service often fails on a single transient error, such as a 503 during a deploy or a dropped connection, and the whole run is marked failed. A person then re-runs by hand and hopes. We will let a Request be retried in a run when it fails with a transient error, with a short backoff between tries, but only for methods that are safe to repeat: GET, HEAD, PUT and DELETE. A POST is never retried unless the person marks it as safe to repeat.

## Decisions within this

- **Only safe methods by default:** GET, HEAD, PUT and DELETE are retried, because repeating them doesn't add a second effect.
- **POST is opt-in:** a Request can be marked as safe to retry, so a payment or an order is never sent twice by accident.
- **Limited and visible:** a small fixed number of tries, with a growing wait, and each retry is listed in the run's output, so it's clear a request needed a second try.
- **Retries are off in the interactive app by default:** a person who presses Send gets the first answer, and can press again.

## Considered Options

- **No retries:** simplest, and the run reports the real first failure, but flaky services fail runs that would have passed.
- **Retry everything:** catches more transient failures, but it can repeat a non-idempotent action.
- **Retry without limit until success:** convenient, but it can hang a CI run for a long time.

## Consequences

- A retry changes how long a run takes, so the timeout in ADR 0064's timing view must show retries as separate attempts.
- Retries must not re-run the built-in functions with new values, or a retried body would differ from the first try, which [ADR 0008](0008-built-in-functions-for-generated-values.md) keeps fixed per send.
- Captures from a failed attempt must be discarded, so only the attempt that succeeded fills a Variable.
