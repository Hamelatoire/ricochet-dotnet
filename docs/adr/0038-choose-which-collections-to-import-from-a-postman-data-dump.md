---
status: accepted (built; the Replace, Copy and Skip choices are not built yet)
---

# Choose which collections to import from a Postman data dump

A Postman data dump holds many collections and environments. Ricochet imports the whole file, so unwanted collections come in and must be deleted by hand.

Ricochet will list the collections and environments inside a Postman data dump, selected by default, and import only the ones the user keeps selected.

## Considered Options

- Import the whole dump: today's behaviour, which forces unwanted collections in.

## Consequences

- The dump is a ZIP that Ricochet must read, so the reader must refuse anything that isn't a valid export.
- Duplicate names follow the rules in ADR 0036.

## Built

- Choosing a `.zip` in Import opens a list of every export in the dump, each ticked. Import brings in only the ticked ones, and nothing else in the dump is read into the workspace.
- `PostmanImporter.ReadDump` (in `PostmanImporter.Dump.cs`) reads the ZIP. It refuses the whole dump, naming the entry, if the file is not a ZIP, holds no files, holds a non-`.json` file, lists a file twice, has an entry that is not a Postman export, or unpacks to more than 100 MB. Folder entries are ignored.
- Duplicate names are numbered as in other imports (for example "Shop API (2)"). The Replace, Copy and Skip choice from ADR 0036 is not built yet.
