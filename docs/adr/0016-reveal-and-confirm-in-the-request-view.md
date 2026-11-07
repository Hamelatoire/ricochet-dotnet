---
status: accepted (built)
---

# Reveal values on hover and confirm actions in place

Three small gaps made the request view harder to trust. A bearer token and an API key value were shown as plain text, while a Basic password was already masked. A `{{variable}}` in the URL gave no hint of what it would send. And clicking Copy on a response gave no sign that anything had happened. We fix all three, keeping secrets hidden and feedback close to the control that caused it.

## Decisions

- **Mask every credential field the same way:** the bearer token and API key value use the same mask as the Basic password. A value typed into these fields goes to secure storage ([ADR 0004](0004-secrets-in-os-storage.md)), so it should not be readable over the shoulder while it is being typed.
- **Hovering a variable shows its value, except for Secrets:** a plain variable shows `{{name}} = value`, so you can check what a request will send without opening Environments. A Secret shows only that it is hidden, because a value shown on hover can be seen by anyone at the screen, and the page must not reveal what the mask hides. Built-in functions such as `{{$guid}}` show nothing, because they produce a new value each time the request is sent ([ADR 0008](0008-built-in-functions-for-generated-values.md)).
- **Copy confirms on the button itself:** the label changes to "Copied" for 1.5 seconds and then returns to "Copy". A toast or a status line would take attention away from the response, and the button is where the eye already is.

## Considered Options

- **Show Secrets on hover too:** convenient, but it exposes a value that the mask exists to hide, so we rejected it.
- **A toast for copy feedback:** would be consistent with other notices, but it adds a separate element for a single action, so we rejected it.

## Consequences

- **The hover value is read live:** it follows the active Environment and Collection at the moment of hovering, and it does not include values captured from earlier responses.
- **Copy feedback does not report failure:** "Copied" appears only after the clipboard write returns. If the write fails, no label changes and no error is shown.
