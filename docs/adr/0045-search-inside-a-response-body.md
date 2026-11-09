---
status: accepted
---

# Search inside a response body

A large response, such as a list of a few thousand items, can only be scanned by eye. There is no way to find a value in it except scrolling, so people copy the body into another tool to search it. We will add a find bar to the response view that highlights every match, steps between matches, and shows how many there are. Searching changes nothing in the Request or the saved body, and it works on the body as it is shown, pretty-printed or raw.

## Decisions within this

- **Find is the usual shortcut:** Ctrl+F opens the bar when the response view has focus, and Escape closes it, as the rest of the app does.
- **Matches are counted and stepped through:** the bar shows "3 of 41" and moves with Enter and Shift+Enter, so the position in a long body is never lost.
- **Case-insensitive by default:** a toggle switches to case-sensitive, because most searches are for a word or a key, not an exact capital.
- **The search is of the displayed text:** matches are found in what is shown, so a pretty-printed body finds a key as it appears on screen.

## Considered Options

- **Copy the body to another tool:** no work, but it leaves the app for the most common response task.
- **A regular expression search:** more powerful, but most searches are plain text, and a pattern box adds a choice that many people won't use. It can come later as a toggle.
- **Search the raw bytes only:** exact, but it doesn't match what someone sees in a pretty-printed body.

## Consequences

- The response view is an AvaloniaEdit control, which already has a search panel, so most of this is wiring and styling rather than new search code.
- A very large body may be slow to search, so the bar must stay responsive while it searches.
- The find bar needs its own colours in both themes, following [ADR 0013](0013-light-and-dark-theme.md), and its own screenshots.
