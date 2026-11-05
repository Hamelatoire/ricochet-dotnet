---
status: proposed
---

# Support a light and a dark theme, following the system by default

The app is dark only, set once in `App.axaml`, and [ADR 0007](0007-personal-settings-page.md) leaves theme out until there is a second one. Many people work in bright rooms or set their whole system to light, and an app that ignores that feels foreign next to their other tools. We will add a light theme, and follow the operating system's light or dark setting by default. The choice of theme is a personal preference, so it belongs on the settings page, not in the Workspace, and a teammate's theme never changes how a shared file looks to you.

## Decisions within this

- **Follow the system first:** a new install matches the operating system, so nobody has to find a setting to get the theme they already use.
- **A setting overrides the system:** System, Light or Dark, on the settings page, stored in `state.json` with the other personal settings.
- **Every colour is a theme resource:** no colour is written in a view. Views refer to named resources, and each theme defines all of them, so a missing colour is a build problem rather than a visual one.
- **Method colours are part of the theme:** the colours that tell `GET` from `POST` must stay readable on both backgrounds, so they are defined per theme rather than in `MethodBrushConverter`.

## Considered Options

- **Dark only:** the least work and the current state, but it ignores a setting many people use, and it leaves the second theme to be designed later under pressure.
- **A switch in the title bar:** quick to reach, but it's another control to design, and a personal preference is a better fit for the settings page.
- **Follow the system only, with no override:** simpler, but someone who wants dark on a light system, or the reverse, has no way to choose.

## Consequences

- The colours are spread through `App.axaml`, `MainWindow.axaml` and `MainWindow.axaml.cs`, and `MethodBrushConverter` has its own colours. These must move into theme resources before a light theme can look right.
- The response viewer is an AvaloniaEdit control with its own highlighting, so its colours need a light version as well.
- Visual pull requests will need screenshots in both themes, which doubles the screenshots that [CLAUDE.md](../../CLAUDE.md) asks for. The screenshot scripts must set the theme explicitly so that the captures don't depend on the machine.
- Following the system needs the app to react when the setting changes while it's open.
- Until the settings page exists ([ADR 0007](0007-personal-settings-page.md) is still proposed), the theme can only be chosen by following the system, which is the default anyway.
