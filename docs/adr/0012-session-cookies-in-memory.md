---
status: accepted
---

# Keep cookies between Requests, in memory only

Ricochet doesn't currently keep cookies: `Set-Cookie` is shown in the response and nothing is sent back, so an API that logs in with a session cookie needs its `Cookie` header copied by hand. We will keep cookies automatically for each Environment, the way a browser does, and hold them only in memory. A session cookie is a credential, so like a Capture ([ADR 0006](0006-declarative-tests-not-scripts.md)) it should never reach a file, and sending the login again after a restart is the same step people already take for captured tokens.

## Decisions within this

- **One set of cookies per Workspace and Environment:** switching from staging to production, or between two test users on one host, must not send the other Environment's session.
- **A CLI Run starts empty:** cookies last for that Run only, so a result never depends on an earlier one.
- **A `Cookie` header written on the Request wins:** the stored cookies aren't sent with it, so a Request written by hand behaves exactly as it says.
- **Cookies are visible and clearable:** a response's **Cookies** tab shows what it set, and the current cookies can be viewed and cleared. Automatic behaviour that can't be seen would make failures hard to explain.
- **A Request can opt out:** one setting on the Request stops it sending or storing cookies, for testing what happens when logged out. It's omitted from the file when off, as Assertions are.
- **History keeps hiding them:** `Cookie` and `Set-Cookie` stay hidden in History, as they are today.

## Considered Options

- **Keep having no cookies:** every Request stays independent and simple, but login flows with session cookies remain painful, and these are common in APIs behind a website.
- **Store cookies in the OS secure storage:** sessions survive a restart, but long-lived sessions then sit on disk, and sessions that have expired cause confusing failures.
- **Store cookies in the Workspace:** rejected, because committing one would share a live session with everyone who can read the repository.
- **A global set of cookies:** simplest, but one Environment's session would be sent in another's Requests whenever hosts match.

## Consequences

- This changes behaviour: a Request that used to be sent with no cookies can start sending them, so the Request tab must show exactly what was sent.
- The app and the CLI can differ, because the app may hold a cookie from earlier sending while a Run starts empty. A Run that needs a session must include its login.
- A login often sets its cookie on a redirect, and redirects are currently followed inside the HTTP client, where those responses can't be seen. So either the client must follow redirects itself, or each set of cookies needs its own HTTP handler. This must be settled alongside the plumbing in [ADR 0007](0007-personal-settings-page.md).

## What was built

- `RequestSender.Configure` takes an optional `keepCookies` flag, off by default. When it's on, the sender keeps one in-memory `CookieContainer` for its requests, and the handler stores `Set-Cookie` from every redirect hop and sends the cookies back on each hop. Redirects are settled this way: the HTTP client keeps the cookies itself, so no client-side redirect following was needed.
- A Request that writes its own `Cookie` header is sent through a client without the store, so the header goes out exactly as written. Its `Set-Cookie` is not kept.
- `RequestSender.ResetCookies()` forgets every kept cookie. Changing settings with the flag on keeps them, and turning the flag off drops them.
- Nothing is written to a file. The cookies live only in the sender.

Not built yet: the app and the CLI don't turn the flag on, and nothing calls `ResetCookies` when a Collection changes. The per-Request opt-out (it needs a field on the Request in `Models.cs`), the **Cookies** tab, and the Environment scoping are also still to do.
