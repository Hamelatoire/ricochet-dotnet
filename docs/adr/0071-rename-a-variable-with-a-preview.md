---
status: proposed
---

# Rename a Variable across the Workspace, with a preview

Renaming a Variable, such as `base_url` to `api_base`, today means finding every Request that uses it and editing each one. A missed one fails only when it's sent, with a placeholder left unresolved. We will let a Variable be renamed once, and show a preview listing each Request, header and Capture that will change, before anything is written. Confirming the preview applies the rename to every listed place in one step.

## Decisions within this

- **Preview first:** the list of changes is shown before any file is written, with the place where each change would happen.
- **Only exact names change:** a rename matches `{{name}}` in full, so `{{base_url_v2}}` is left alone.
- **One step, one undo:** the rename is one change, which the undo in [ADR 0047](0047-undo-and-redo-in-request-edits.md) can reverse.
- **Secrets are renamed like other Variables:** a Secret keeps its value in the store under [ADR 0004](0004-secrets-in-os-storage.md), and only its name in the Workspace changes.

## Considered Options

- **Rename by hand:** no work, and what exists, but it's error-prone for a Variable used in many places.
- **Rename without a preview:** faster, but a wrong rename would change files the person didn't expect.
- **Keep the old name as an alias:** avoids breaking, but two names for one value confuses the model in [ADR 0019](0019-prompt-for-a-variable-when-a-request-is-sent.md)'s prompts.

## Consequences

- The rename must touch only files the app read successfully, as the rule for saves requires, and it must report a file it couldn't change.
- Renaming across a Collection shared by a team changes files other people pull, so the preview should say that changes will show in the next commit.
- The rename must also update the example responses in [ADR 0023](0023-save-example-responses-on-a-request.md) only where they hold the placeholder text.
