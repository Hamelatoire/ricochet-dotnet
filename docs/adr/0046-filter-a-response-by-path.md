---
status: proposed
---

# Filter a JSON response down to the part you need

A JSON response can be hundreds of lines when only one field matters. Scrolling through it, or copying it into a tool that can select a field, slows down every check someone makes. We will let a response be narrowed to a path such as `data.items[0].id`, showing only that part, and let the same path be copied from the tree. The path syntax is the one [Captures](../../GLOSSARY.md) already use, so a path that works in a Capture works here.

## Decisions within this

- **One path syntax, shared with Captures:** a path that saves a value into a Variable also finds it in the view. Learning it once is enough.
- **A filter changes only the view:** the response, its saved body and History keep the whole body. A filter is removed when the next response arrives, unless the person asks to keep it.
- **Copy a path from the tree:** clicking a key in the response tree copies its path, so the filter doesn't have to be typed.
- **Errors say what was wrong:** a path that matches nothing says so, and shows the closest key found, rather than an empty pane.

## Considered Options

- **Type the path only, with no tree:** simpler, but people have to know the structure before they can filter it.
- **A full JSONPath or query language:** more power than most checks need, and a second syntax next to Captures.
- **Filter the saved body on disk:** it would change what was received, which the History rules in [ADR 0008](0008-built-in-functions-for-generated-values.md) and the body-saving rule in [ADR 0023](0023-save-example-responses-on-a-request.md) don't allow.

## Consequences

- The response tree needs its own control, separate from the text view, so the two must keep the same selection when switched.
- Non-JSON responses have no tree, so the feature is hidden for them, and XML or HTML is left out of this decision.
- Paths on a large array need a limit, so that a filter over thousands of items stays fast.
