---
status: proposed
---

# Save example responses on a request

Ricochet keeps only the last response, in memory, and nothing about a response is saved with its request. Saved examples of expected responses help with documentation, with checking changes, and as input for mock servers (ADR 0027).

A request can keep named example responses, saved by the user. They are stored in the request file, so they are shared with the collection, and they can later feed mock servers (ADR 0027).

## Considered Options

- Keep history only: rejected, because examples are meant to be shared.
- Store them in a separate file: rejected, because one file per request keeps diffs small (ADR 0003).

## Consequences

- Examples may contain real data. The response must go through the same masking as History before it is saved.
- Adds a field to request files, which stays empty when unused.
