---
status: proposed
---

# Stream large response bodies to disk above a limit

The app reads a whole response into memory before showing it, so a response of several hundred megabytes, such as a database export or a large file, uses that much memory and can freeze the window. [ADR 0023](0023-save-example-responses-on-a-request.md) and the save-body button let the person keep a body in a file, but only after it has been read into memory. We will stream a body above a set size straight to a file, show its size and the first part on screen, and keep the body out of memory and History beyond the first part.

## Decisions within this

- **A size limit, set once:** bodies below the limit are read as now. Above it, the body is streamed to a file in the personal folder, and the response view says so.
- **The on-screen part is capped:** only the first part of a large body is shown, with a notice and a button to open the file.
- **History keeps a reference, not the body:** the History entry records the file's location and size, not its contents, so History stays small.
- **The file is not written into the Workspace:** downloaded bodies are personal, following [ADR 0003](0003-git-friendly-workspace-files.md).

## Considered Options

- **Keep reading everything into memory:** simple, but a large response can stop the app.
- **Refuse bodies above a limit:** safe, but people do sometimes need the data.
- **Always stream:** uniform, but small responses would lose the in-app view that most people use.

## Consequences

- Streamed bodies can't be pretty-printed or searched in the response view, so the search in ADR 0045 must say so for those.
- The downloaded files accumulate, so the personal folder needs a limit and a way to clear them.
- Secret masking from ADR 0076 must apply to a streamed body saved on disk, as it applies to other saved output.
