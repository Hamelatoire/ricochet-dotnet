---
status: proposed
---

# Compare a response with the previous one

Checking whether an API change broke something usually means looking at two responses side by side and finding the difference by eye. The History keeps the earlier sends, but the response view can only show one at a time. We will let a person pick a second response, from History or the last send, and show the two side by side with the differences highlighted, for the body and the headers.

## Decisions within this

- **Pick any two responses from History:** the comparison works on responses already recorded, so no new request is sent to compare.
- **Highlight changed lines and values:** added, removed and changed lines are marked in each side, and changed header values are marked in the header table.
- **Secrets are not compared:** a header value that holds a Secret is masked on both sides, so a comparison can't reveal it, as [ADR 0016](0016-reveal-and-confirm-in-the-request-view.md) requires.
- **The comparison is view-only:** it changes neither response, and nothing is saved from it.

## Considered Options

- **Export both responses and use a diff tool:** works, but it means leaving the app and saving files that hold response data.
- **Compare only the bodies:** simpler, but a changed header, such as a new rate limit, is often the change that matters.
- **Compare automatically with each send:** always up to date, but it would show a difference on every send and become noise.

## Consequences

- History keeps responses only as far as its limits allow, so a comparison may be impossible for old sends, and the view must say so.
- A pretty-printed body needs to be compared after formatting, or every line would differ on whitespace alone.
- The side-by-side view needs room, so on a narrow window it must stack the two responses rather than squeeze them.
