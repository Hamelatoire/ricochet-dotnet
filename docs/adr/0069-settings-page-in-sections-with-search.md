---
status: proposed
---

# Group the settings page into sections with a search box

The settings page from [ADR 0007](0007-personal-settings-page.md) has grown one control at a time, and its options sit in one long list with no grouping. Finding the proxy setting or the theme means scrolling past everything else. We will group the settings into sections such as Appearance, Requests, Network and Privacy, show a section list beside them, and add a search box that filters the settings by name.

## Decisions within this

- **Sections follow what the setting changes:** Appearance holds the theme, Requests holds timeout and redirects, Network holds the proxy and certificates from [ADR 0026](0026-proxy-and-client-certificates-for-requests.md), and Privacy holds the history and data options.
- **Search matches the setting's name and its help text:** typing "certificate" finds the client certificate option even if it sits in another section.
- **Each setting keeps its meaning:** grouping changes where a setting is shown, not what it does or where it is saved.
- **Nothing moves to the Workspace:** settings stay personal, as [ADR 0007](0007-personal-settings-page.md) requires.

## Considered Options

- **Keep the single list:** no work, but it gets longer with each new option.
- **Tabs across the top:** clear, but a tab for each group uses horizontal space and hides the other groups.
- **Separate windows for each group:** possible, but it adds windows to manage for a small amount of content.

## Consequences

- Moving settings to sections changes where they appear in the file for screenshots, so the README screenshot of the settings page must be retaken in both themes, as [CLAUDE.md](../../CLAUDE.md) requires.
- A new setting must be assigned a section, which a test can check so no setting is orphaned.
- The search must include the help text of each setting, which then has to be kept clear enough to be found by search.
