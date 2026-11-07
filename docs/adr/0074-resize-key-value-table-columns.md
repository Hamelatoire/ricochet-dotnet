---
status: proposed
---

# Resize the columns of key-value tables

Headers, query parameters and form fields are all edited in key-value tables with fixed column widths. A long header value is cut off and has to be clicked into to read, and a short key takes as much room as a long one. We will let a person drag the boundary between the key and value columns in any key-value table, and remember the width for that table kind on this machine. The width is a view setting, so it does not change the Request.

## Decisions within this

- **One boundary, dragged:** the key and value columns are split by a boundary that can be dragged, the way columns work in a file list.
- **Remembered per table kind:** headers, query parameters and form fields each keep their own width, saved with the other layout settings in [ADR 0043](0043-remember-panel-sizes-and-sidebar-state-per-person.md).
- **Minimum widths are kept:** neither column can shrink below a size that still shows a few characters, so the table never becomes unusable.
- **The Request is unaffected:** the width is never written to the Request file, following the rule that a file holds only what changes the Request.

## Considered Options

- **Keep fixed widths:** simple and predictable, but long values stay hard to read.
- **Let the value column wrap onto several lines:** shows everything, but a table of many rows becomes tall, against [ADR 0042](0042-denser-layout-and-larger-controls.md).
- **Show the full value in a tooltip:** useful for one value, but it doesn't help when comparing several.

## Consequences

- The table control is shared with the variable editor, so a change must be checked in every table that uses it.
- A drag handle needs a keyboard equivalent, which the audit in [ADR 0052](0052-keyboard-and-screen-reader-audit.md) must cover.
- Widths saved for a small window must still fit a larger one, so they are stored as proportions, not pixels.
