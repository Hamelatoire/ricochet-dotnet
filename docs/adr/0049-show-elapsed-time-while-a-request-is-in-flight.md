---
status: proposed
---

# Show elapsed time while a Request is in flight

Sending a Request shows a Send button that turns into Cancel, and nothing else. A slow request looks the same as a stuck one, and a person has to guess how long to wait before cancelling. We will show the elapsed time next to Cancel, count up once a second, and show the configured timeout beside it, so waiting has a visible end. The Cancel control stays where it is.

## Decisions within this

- **Elapsed time counts up:** "2.4 s of 120 s" shows the time spent and the limit, so it's clear the request will end.
- **Time is shown from the first moment:** it appears as soon as the Request is sent, not after a delay, so a press never looks ignored.
- **Cancel keeps its place:** it stays the same control, so the keyboard shortcut and the Send-or-Cancel binding don't change.
- **The timer is cosmetic:** it doesn't change the timeout, which is still set in Settings and enforced by the sender, as [ADR 0007](0007-personal-settings-page.md) set out.

## Considered Options

- **A spinner only:** quieter, but it doesn't tell someone how long is left.
- **A progress bar:** it needs a known total, and a request's size and speed are unknown in advance, so it would mislead.
- **Show a notice after a few seconds:** it adds a delay, and the timer is already visible at no cost.

## Consequences

- The view model must update the displayed time once a second without blocking the send, so the timer must not hold the request's thread.
- The timeout applies to the whole request, so the display shows the time spent against it, which can differ from the time a bulk run takes per row ([ADR 0017](0017-parallel-bulk-runs.md)).
- Screen readers should not announce the timer every second. It should be announced only when the request finishes.
