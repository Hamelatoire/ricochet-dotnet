---
status: proposed
---

# Export and import a whole workspace as a ZIP

Moving a workspace to another machine, or sharing it with someone who does not use git, means copying its folder by hand, which is easy to do wrong.

Ricochet may export the shareable workspace folder as one ZIP, and import a ZIP into a chosen folder. Personal files, History and secrets are never included.

## Considered Options

- Copy the folder by hand: stays available, but is easy to get wrong.

## Consequences

- A ZIP is a single file that can be shared, so the secret exclusion must be tested.
- Imports must not overwrite an existing workspace (ADR 0002).
