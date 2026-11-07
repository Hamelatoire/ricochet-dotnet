---
status: proposed
---

# Switch the Environment from the request view

Changing the active Environment means opening the sidebar or a menu to find it. Someone who sends the same Request to staging and then production does this many times a day, and each switch takes several clicks away from the URL they are editing. We will put an Environment selector in the request view's header, next to the send button, so the switch is one click from where the Request is being sent.

## Decisions within this

- **The selector shows the active Environment:** the name is visible at all times, so a send always shows which server it goes to.
- **Choosing an Environment changes only the active one:** it doesn't edit any Environment or Variable, and it is the same choice that the Settings and sidebar already make, as [ADR 0007](0007-personal-settings-page.md) describes.
- **Secrets stay hidden:** the selector shows names only, never the value of a Secret, in line with [ADR 0004](0004-secrets-in-os-storage.md).
- **A keyboard shortcut moves through Environments:** the palette in [ADR 0040](0040-keyboard-shortcuts-and-a-command-palette.md) gets a "next Environment" entry, so the switch doesn't need the mouse at all.

## Considered Options

- **Keep the switch in the sidebar:** no change, but the switch stays away from the request.
- **Duplicate the whole Environment editor in the header:** too much for the header, and editing belongs with the Environment.
- **Automatic switching by Request name:** convenient, but it would send to a server the person didn't choose, which is exactly the risk the selector exists to prevent.

## Consequences

- The selector must update when an Environment is added, renamed or deleted, so it can't show a stale name.
- The header gets one more control, which must fit at the smallest supported width.
- A Request that uses an Environment-only Variable shows its value once the Environment changes, which must be reflected in the chips from [ADR 0009](0009-chips-over-plain-text.md).
