---
status: proposed
---

# Plain messages when a send fails

When a request fails, the message is usually a technical one, such as a socket error or a certificate name mismatch, with no hint of what to do. The person has to know the difference between a DNS failure, a refused connection and a timeout to fix it. We will give each common failure a short plain message that says what happened and one thing to try, and keep the technical detail underneath for those who want it.

## Decisions within this

- **Name the cause in plain words:** "The server name could not be found" instead of a resolver error code.
- **Suggest one next step:** "Check the URL and your network connection" or "Raise the timeout in Settings", so the message is an answer, not a dead end.
- **Keep the technical detail one click away:** the original error is shown under a "Details" toggle, so a person reporting a problem can copy it.
- **Match the CLI's wording:** the same failure gets the same plain message in the CLI output, so a CI log and the app agree.

## Considered Options

- **Show only the technical error:** precise, but it sends people to a search engine.
- **Replace every error with a generic "request failed":** tidy, but it hides the one fact that would help.
- **Translate every error code into a long explanation:** thorough, but long messages are read less often than short ones.

## Consequences

- Each mapped error needs a test, so that a change in the sender doesn't leave a message out of date. The Core tests in [ADR 0044](0044-test-view-models-and-headless-views.md)'s style can cover this.
- Unknown errors keep the technical message, so no failure is hidden behind a wrong guess.
- The plain messages must be included in the text-resource work in [ADR 0054](0054-prepare-interface-text-for-translation.md).
