---
status: proposed
---

# Use one icon set, with a name for every icon

The app's icons were added one at a time, and they differ in weight, size and meaning: some are filled, some outlined, and a few have no name a screen reader can use. [ADR 0042](0042-denser-layout-and-larger-controls.md) enlarges the icon targets, which makes these differences more visible. We will use one icon set for the whole app, with one weight and one size grid, and every icon-only button will have a tooltip and an accessible name. An icon that is not clear on its own will get a text label instead.

## Decisions within this

- **One set, one weight:** the icons come from a single set, drawn at the same stroke width, so they look like one design.
- **Every icon is named:** each icon-only control has both a tooltip and an accessible name, which the audit in [ADR 0052](0052-keyboard-and-screen-reader-audit.md) checks.
- **Unclear icons get a label:** if an icon's meaning is not clear without help, it gets a short text label beside it, instead of relying on a tooltip that appears only on hover.
- **Colour follows the theme:** icons use the theme's colour tokens from [ADR 0013](0013-light-and-dark-theme.md), so they are never a fixed colour.

## Considered Options

- **Keep the icons as they are:** no work, but the inconsistency stays and more icons will be added unevenly.
- **Text labels everywhere:** clearest, but it takes space that [ADR 0042](0042-denser-layout-and-larger-controls.md) is trying to keep tidy.
- **A third-party icon font:** easy to use, but it adds a dependency and a licence to check for a small icon set.

## Consequences

- Replacing the icons changes their look everywhere, so each change needs screenshots in both themes, as [CLAUDE.md](../../CLAUDE.md) requires.
- The icon set must be checked under Native AOT, as an embedded resource rather than a font loaded at run time.
- A list of every icon and its name must be kept in the repository, so that a new icon is added in the same style.
