---
status: proposed
---

# Notify when a long run finishes in the background

A bulk run or Collection run can take many minutes, and the person often switches to other work. They return to the window to check whether it finished, and sometimes find it finished long ago or stopped early. We will show a notification from the operating system when a run that took longer than a set time finishes, while the window is in the background. The notification names the result and the count of passed and failed items, and clicking it brings the window to the run's log.

## Decisions within this

- **Only for runs that take a while:** a run under a threshold, one minute by default, doesn't notify, so short runs don't interrupt.
- **Only when the window is in the background:** a person who is watching the run sees no notification.
- **Local notification only:** it is shown by the operating system from this machine, and nothing is sent elsewhere, in line with [ADR 0005](0005-local-first.md).
- **The notification can be turned off:** the choice sits on the settings page from [ADR 0007](0007-personal-settings-page.md).

## Considered Options

- **No notification:** no interruption, but people keep checking the window.
- **Notify for every run:** simple, but it trains people to ignore notifications.
- **A sound on finish:** easy to miss in a noisy room, and sound is harder to turn off per run.

## Consequences

- Each operating system has its own notification API, so this needs platform code behind one interface, as [ADR 0004](0004-secrets-in-os-storage.md) does for secrets, and it must be tested on all three.
- A notification can be dismissed or missed, so the result must also stay in the app's history for that run.
- Notifications need the app's own permission on macOS, which the first-run flow must explain.
