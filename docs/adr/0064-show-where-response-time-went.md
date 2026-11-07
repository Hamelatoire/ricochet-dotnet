---
status: proposed
---

# Show where a response's time was spent

A response reports one time, and when a request is slow, that number doesn't say whether the delay was finding the server, connecting, waiting for the first byte, or downloading the body. Each cause points to a different fix. We will show a breakdown of the time for each response: name lookup, connection, time to the first byte, and download. The breakdown is measured on the machine that sent the request and is shown only in the response view.

## Decisions within this

- **Four phases, in plain names:** "Name lookup", "Connect", "Waiting for the server" and "Download", so each phase says what it means.
- **Measured, never estimated:** each phase is timed when it happens. A phase that didn't happen, such as a reused connection, shows as zero rather than a guessed value.
- **Shown in the response view only:** it is not added to History or the saved examples, which keep their current contents.
- **Available for every response:** the breakdown appears whether or not the request was slow, so it can be compared between sends.

## Considered Options

- **Show only the total:** what exists, and enough when a request is fast, but it doesn't say why one is slow.
- **Log the detail to a file:** useful for an investigation, but a person looking at one slow request wants it on screen.
- **Send the timing to a monitoring service:** rich data, but [ADR 0005](0005-local-first.md) rules out sending anything the person did not ask for.

## Consequences

- The timings need the HTTP client to report each phase, which may need a custom handler, so the approach must be checked under Native AOT.
- Timings on a reused connection differ from a new one, and the view must say so rather than compare them as equals.
- The timing numbers are measured on this machine, so they include local delays such as a proxy, which the proxy setting in [ADR 0026](0026-proxy-and-client-certificates-for-requests.md) can change.
