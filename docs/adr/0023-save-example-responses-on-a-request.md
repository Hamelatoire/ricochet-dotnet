---
status: accepted
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

## Built

- `ApiRequest.Examples` holds `ResponseExample` records (name, status, headers, media type, body). It is null when empty, so Requests without examples are unchanged on disk.
- `ResponseExamples.Create` in Core masks the saved response: sensitive headers are hidden as in History, and secret values (auth, sensitive headers, Secret Variables, captures) are masked wherever they were echoed. Truncated and binary responses are refused.
- The App saves the last response from the response panel, lists examples on the Request under the Examples tab, shows one in the response panel, and removes one.
