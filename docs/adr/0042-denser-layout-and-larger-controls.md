---
status: proposed
---

# A denser layout with larger controls

The current layout spaces rows and panels generously, so a Collection with many Folders fills the sidebar quickly, and the request view has less room for headers and bodies than the window allows. The folder action icons are also small enough to miss, which makes the most common actions hard to find. We will tighten the spacing of lists and panels and enlarge the clickable area of icon buttons, without changing the colours or the overall structure. The change applies everywhere, so no one has to choose between a compact and a comfortable mode.

## Decisions within this

- **Spacing comes from named values:** row height, padding and gaps are theme resources, as [ADR 0013](0013-light-and-dark-theme.md) requires for colours, so the density is changed in one place.
- **Icon buttons get a larger target, not a larger icon:** the icon stays the same size, and the clickable area grows to at least 28 by 28 pixels, so small icons stay tidy and are easy to hit.
- **Text size does not change:** only spacing and targets change, so readability is not traded for room.
- **Each screen is checked before and after:** the sidebar, request view and response view are each captured in both themes, as [CLAUDE.md](../../CLAUDE.md) asks for changes that alter an existing screen.

## Considered Options

- **Keep the current spacing:** no work and no risk, but the crowding stays.
- **A user setting for compact or comfortable:** flexible, but it doubles every screen to test and adds a choice most people won't make, against [ADR 0007](0007-personal-settings-page.md)'s aim of a small settings page.
- **Shrink the text as well:** fits more on screen, but it makes the app harder to read, which goes against the goal of a tool used all day.

## Consequences

- Screenshots are needed for every screen that changes, in both themes, which is the cost [ADR 0013](0013-light-and-dark-theme.md) already accepted.
- A denser list puts more rows on screen, so any list virtualisation needed for large Collections becomes more important.
- Touch and high-contrast use are not tested by the current automation, so they need checking by hand before this ships.
