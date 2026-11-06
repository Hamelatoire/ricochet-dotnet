---
status: accepted
---

# A settings page for personal settings only

Some behaviour is currently fixed in code, such as the two-minute request timeout, and people will reasonably want to change it. We will add a settings page that holds only personal preferences for this person on this machine, stored in the personal `state.json` and never in the Workspace. Keeping them out of the Workspace means a teammate's preference can't change how your requests behave, and a `git pull` can't change your settings.

## Decisions within this

- **A full page, not a dialog:** it replaces the request and response panels and is opened from the sidebar, so it has room to grow, and the Collection tree stays in view.
- **Changes apply immediately:** there is no Save button, matching how the rest of the app keeps state. Risky settings say what they do right beside the control.
- **Only settings with a real use:** every option adds something to test and explain, so one is added when someone needs it, not because it might be useful.

Environment variables are not settings: they are Workspace data shared with the team, so they have their own Environments page.

## First settings

| Setting | Default |
|---|---|
| Request timeout | 120 seconds |
| Follow redirects | On |
| Verify TLS certificates | On |
| Largest response to keep | 100 MB |
| Wrap long lines in responses | On |
| Workspace folder | Moved here from the sidebar's **Change…** |

Theme is left out until there is a second theme to choose.

## Considered Options

- **Settings file edited by hand:** the least to build, but hard to discover, and it offers no guidance on risky options.
- **Settings stored in the Workspace, shared by the team:** would keep behaviour consistent across a team, but one person's change would affect everyone. If shared settings prove necessary, such as a timeout for one API, they should be a separate setting on the Collection.

## Consequences

- **The CLI ignores this page:** a CI run must not depend on what is set on one machine, so it uses only its own options and defaults. A request can therefore behave differently in the app than in CI, for example with a shorter timeout. The CLI will need its own options for anything CI must control, such as certificate verification.
- **Turning off certificate verification is dangerous:** it exposes credentials to interception, so while it's off the app must show a visible warning at all times rather than only on the settings page.
- **Timeout and redirect changes need new plumbing:** the HTTP client is created once at startup, so it must be rebuilt when these change, without disturbing a request in progress.
- **An ADR 0005 commitment still holds:** a future setting that causes network traffic, such as checking for updates, must be off by default.
