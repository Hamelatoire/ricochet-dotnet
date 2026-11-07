---
status: proposed
---

# Test view models and headless views

The tests cover `Ricochet.Core` only, and [CLAUDE.md](../../CLAUDE.md) says to check App changes in the running app. That works for a screen, but it leaves nothing to catch a regression in the logic behind the screens, such as which Requests a tab shows after a rename or whether a bulk run's rows are right. We will test the view models directly, and test a small set of views with Avalonia's headless test host, which draws a window without a display. This keeps the running app as the final check, and makes the common cases automatic.

## Decisions within this

- **View models are tested first:** most behaviour lives in `MainViewModel` and its helpers, and these run with no UI at all, so they are the first target.
- **A separate test project for the App:** `tests/Ricochet.App.Tests`, so the Core tests stay fast and free of UI packages, as the current `dotnet test --project` commands expect.
- **Headless tests cover a few views:** the sidebar tree, the key-value editor and the request view, since those are where the bindings and the new `FocusBehavior` are most likely to break.
- **Screenshots remain the proof for a visual change:** headless tests check structure and bindings, not colour or spacing, so [CLAUDE.md](../../CLAUDE.md)'s screenshot rule is unchanged.
- **Tests use temporary Workspaces:** each test writes to a temporary folder and sets `RICOCHET_DATA`, so no test touches real data.

## Considered Options

- **Rely on the running app only:** no new packages or projects, but regressions are found by hand, and the same checks are repeated for every change.
- **Drive the app with screen automation:** the existing PowerShell scripts do this, but they depend on the screen, are slow, and break when the layout moves.
- **Test everything through the headless host:** thorough, but the view model tests would then run slower and depend on a window they don't need.

## Consequences

- A new test project and the Avalonia headless package are added. Under [ADR 0005](0005-local-first.md) this is only a build dependency and adds no traffic.
- The headless host needs the same Native AOT-friendly code as the app, so some test helpers may not run under AOT. Tests run on a normal build, and the AOT publish is still checked separately.
- The CI matrix runs the new project on all three operating systems, so headless behaviour must be the same on each.
