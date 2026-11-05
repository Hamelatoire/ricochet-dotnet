---
status: proposed
---

# Show Variables and functions as chips over plain text

In a long URL or body, `{{base_url}}` is easy to mistype and hard to spot, and nothing says whether it has a value until the Request is sent. We will show each Variable and function as a chip inside the text, and suggest names while typing, in every field that can use them. A chip exists only on screen: the file, the clipboard and the sent Request all use the same plain `{{name}}` text, so git diffs and [ADR 0003](0003-git-friendly-workspace-files.md) are unaffected.

## Decisions within this

- **One editor everywhere:** the URL, body, header values and auth fields move from `TextBox` to AvaloniaEdit, which the response view already uses, so chips and suggestions behave the same in each field.
- **A chip acts as one character:** the caret steps over it and backspace removes it whole. Clicking it shows the text so it can be edited, and copying gives `{{name}}`.
- **A chip shows whether the name resolves:** one with no value in the current scope looks different, so the problem is visible before sending.
- **Suggestions come from what is in scope:** the Variables the Request can see, then the functions from [ADR 0008](0008-built-in-functions-for-generated-values.md). Typing `{{` opens the list.
- **A Secret's value is never displayed:** a chip, tooltip or suggestion shows only its name and that it is a Secret. Other Variables show their value and which scope it came from.

## Considered Options

- **Colour the text, without chips:** much less to build, and it still shows what resolves, but a long function call stays long and can be partly deleted by accident. This is the first step of the work and a good place to stop if chips prove unreliable.
- **Replace each field with tokens stored as structured data:** chips would be simple to build, but the files would no longer be plain text and merges would get harder.
- **Write a new text control:** full control, but it means redoing selection, input methods and undo, which AvaloniaEdit already handles.

## Consequences

- A long header table could hold dozens of editors, so this must be measured against the speed goal. If it's too slow, a row can use a plain view until it is focused.
- Each field's undo, tab order, paste and screen reader behaviour must be retested, because a `TextBox` gave these for free. Pasted text in a single-line field must have its line breaks removed.
- The UI automation scripts used to test the app read `TextBox` values, so they need updating.
- Keeping AvaloniaEdit working under Native AOT becomes more important, since the main editing path now depends on it.
