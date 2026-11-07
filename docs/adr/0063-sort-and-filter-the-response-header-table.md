---
status: proposed
---

# Sort and filter the response header table

A response can carry dozens of headers, and the header table lists them in the order the server sent them. Finding `Cache-Control` or every header that starts with `X-RateLimit` means reading the whole table. We will let the table be sorted by column and filtered by a text box that matches header names and values. The table only changes its own view, and the headers as received, as saved, stay unchanged.

## Decisions within this

- **Sort by name or value:** a click on a column heading sorts, and a second click reverses, which people already expect from tables.
- **Filter matches names and values:** typing "cache" finds the header and any value containing it, so both are found in one step.
- **Original order is one click away:** a third click, or a reset control, returns the server's order, which matters when a header order is meaningful.
- **The view is not saved:** the sort and filter reset when the next response arrives, so they don't leak between Requests.

## Considered Options

- **Leave the table as it is:** works for short lists, which most responses are.
- **Search the whole response for the header:** the search in [ADR 0045](0045-search-inside-a-response-body.md) covers the body, and a header search there is less precise.
- **A separate page for headers:** more room, but it splits information that belongs together.

## Consequences

- The table must keep the server's header order internally, so a sort doesn't lose it.
- Duplicate header names, which HTTP allows, must stay as separate rows after sorting.
- Filtering must not hide a header that was selected for copy, or the copy would fail silently.
