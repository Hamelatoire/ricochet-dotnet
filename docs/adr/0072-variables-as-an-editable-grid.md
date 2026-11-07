---
status: proposed
---

# Edit Variables as a grid, with one column per scope

Variables are edited one scope at a time, so seeing that `base_url` is set in Globals, overridden in a Collection and different again in an Environment means switching between three screens. We will show the Variables as one grid, with a row per name and a column per scope, so the value in each scope and the one that wins are visible on one screen. Values are edited in the grid, and a cell shows which scope it comes from.

## Decisions within this

- **One row per name, one column per scope:** a person sees all values of one Variable together, and the one that applies is highlighted.
- **The winning value is marked:** a cell that wins over the others is shown distinctly, following the precedence in [ADR 0020](0020-folder-level-variables.md).
- **Secrets show a mask:** a Secret's cell shows that it is set, never its value, in line with [ADR 0004](0004-secrets-in-os-storage.md).
- **Adding a Variable asks for its scope:** a new row starts in one scope and the other cells are blank, so a Variable is never added to the wrong place by mistake.

## Considered Options

- **Keep one screen per scope:** what exists, and it keeps each scope simple, but it hides how the scopes relate.
- **A single flat list with a scope label on each row:** fewer columns, but the same name then appears in several rows, which makes overrides hard to see.
- **A full spreadsheet editor:** flexible, but a spreadsheet's rules, such as formulas, are not wanted here.

## Consequences

- A grid with many scopes and many names can get wide, so it must scroll horizontally without losing the name column.
- Editing in a grid needs its own keyboard model, which the audit in [ADR 0052](0052-keyboard-and-screen-reader-audit.md) must cover.
- Scopes that are empty for every name, such as an unused Environment, should be hidden, so the grid stays readable.
