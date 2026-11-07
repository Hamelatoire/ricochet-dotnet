---
status: proposed
---

# Duplicate a Request or Folder

Making a variation of an existing Request means building it from scratch, copying its URL, headers, body and Assertions by hand. The same is true of a Folder full of Requests, which is the common way to set up a second environment's checks. We will add Duplicate to a Request and to a Folder, which makes a copy next to the original with a new name, and copies everything that belongs to it, including its Assertions, Captures and tags.

## Decisions within this

- **The copy is a full copy:** headers, body, auth fields, Assertions, Captures and tags are copied, so the copy behaves like the original from the start.
- **Secrets are referenced, not copied:** a Secret Variable keeps its name, and its value stays in the secure store, as [ADR 0004](0004-secrets-in-os-storage.md) requires, so the copy is never a second place a secret is stored.
- **The copy gets a new name and a new id:** it is named "Name (copy)" and, if the name is taken, a number is added, so it can be told apart at once.
- **Duplicate is undoable:** the copy is one step that the undo from [ADR 0048](0048-undo-a-delete-instead-of-a-second-click.md) can reverse.

## Considered Options

- **Copy and paste in the sidebar:** familiar, but it needs a clipboard that holds Requests, and a paste into a different Collection then needs its own rules.
- **Export and import a Request:** works, but it's a file round trip for a copy in the same Collection.
- **Templates for common Requests:** useful later, but a template is a different concept from a copy of a specific Request.

## Consequences

- Copying a Request must generate a new file name, so it never overwrites an existing file in the Workspace.
- A Folder's copy must keep its Requests' order and its hidden state carefully, since [ADR 0032](0032-hide-a-folder-without-deleting-it.md) hides by Folder.
- The copy is a new file in the Workspace, so it appears in the next commit, which the user should know before they duplicate a large Folder.
