---
status: accepted
---

# Reject control characters in request values

A value that contains a line break or another control character can change what is sent. A Bearer token with a newline in it was once sent as a second header, letting the rest of the value add headers of its own. [PR #94](https://github.com/Hamelatoire/ricochet-dotnet/pull/94) fixed that for headers and auth values. The same risk exists in other places a value reaches the wire, such as the URL, the query string and form fields, which were not checked. We will reject control characters in every value that is sent, at the one place where the request is built, so a new field can't skip the check.

## Decisions within this

- **Check once, where the request is built:** the check runs in the code that turns a Request into bytes, not in each screen, so a field added later is covered without a change.
- **Refuse, don't strip:** a value with a control character fails with a clear message. Removing characters silently would send something other than what the person typed.
- **Covers the URL and query too:** a raw line break in a URL or in a query value is refused the same way as a header.
- **Tabs in values are a separate question:** a tab is allowed where HTTP allows it, and this decision does not change that.

## Considered Options

- **Check each field in the view:** catches typing, but a pasted value or a value from a file or a Variable gets past it.
- **Percent-encode control characters:** safe on the wire, but it changes the value the person meant, so the server receives something unexpected without warning.
- **Trust the HTTP library:** the library refuses some cases, but not all, and it can change between versions.

## Consequences

- A value that was sent before with a stray newline now fails, which is the point, but it can surprise someone who relied on it. The failure message must name the field.
- The check must run on resolved values, after Variables are applied, because a Variable may hold the line break.
- The rule is covered by tests in the Core test project, one per field kind.
