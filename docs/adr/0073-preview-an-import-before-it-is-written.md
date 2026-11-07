---
status: proposed
---

# Preview an import before anything is written

Importing a Postman or Insomnia export writes Collections into the Workspace at once, and the report of what couldn't be carried over comes after. A person who imports a large export can't see what will be created, what will be skipped, or what will be changed in a Collection they already have. We will show a preview of the import first: each Collection and Request that will be written, each one that will be skipped with its reason, and each existing Collection that will be changed. The import starts only when the person confirms.

## Decisions within this

- **Nothing is written before confirming:** the preview is computed in memory, so cancelling it leaves the Workspace as it was.
- **Skips come with a reason:** each skipped item names what couldn't be carried over, as the importers already report, so the preview repeats the same list the report gives.
- **Changes to existing Collections are shown:** an import that would overwrite a Collection with the same name says so, and the person can choose a new name or cancel.
- **Matches the choice in [ADR 0038](0038-choose-which-collections-to-import-from-a-postman-data-dump.md):** the preview lists the Collections to choose from, so the choice is made with the full picture.

## Considered Options

- **Import, then show the report:** what exists, but a mistake is found after the files are written.
- **Import to a temporary folder, then merge:** safe, but it adds a merge step with its own conflicts.
- **Confirm only when a Collection would be overwritten:** less friction, but a skipped item would still be unseen.

## Consequences

- The import must compute the full result without writing, so the importers need a mode that returns the plan, which fits the Core-holds-behaviour rule in [CLAUDE.md](../../CLAUDE.md).
- A large export takes time to plan, so the preview must show progress rather than appear frozen.
- The preview should use the same wording as the importer report, so a person doesn't have to learn two sets of messages.
