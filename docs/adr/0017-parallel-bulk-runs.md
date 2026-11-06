---
status: accepted
---

# Send bulk-run ids in parallel, with a thread count

[ADR 0015](0015-bulk-runs-with-resume.md) sent bulk-run ids one at a time, to avoid overloading the API and to keep the log easy to follow. Long lists are slow that way, and the API under test is usually able to take more load than that. We will let the user choose how many ids are sent at once, with one thread as the default that matches the old behaviour.

## Decisions within this

- **The user picks the thread count:** a number from 1 to 16 on the bulk page, default 4. One thread keeps the sequential behaviour from ADR 0015. The value is kept for the session, not saved to personal state.
- **Results arrive in completion order, one at a time:** the log and the saved progress are written from one result at a time, so they are never interleaved. Log lines can appear out of pasted order.
- **Progress means the leading run of finished ids:** "last id processed" is the last id in the pasted list such that every id before it has finished. Resume continues after that id. An id that finished early but sits after an unfinished one is sent again on Resume. Re-sending is preferred to skipping an id that never got a response.
- **Stop starts no new ids:** ids already in flight finish, as in ADR 0015, so no id is left half sent.
- **No retries, as before:** a failed id is logged and the run continues.

## Considered Options

- **Parallelism fixed in code:** simpler, but it can't match the speed of different APIs, and the user has no way to protect a fragile one.
- **Track progress as a set of finished ids:** precise, but the saved state grows with the list and is harder to read. The leading-run rule needs only one id.
- **Ask the user for an order-preserving log:** would make the log match the pasted order, but results would wait behind the slowest id. Completion order keeps the run moving.

## Consequences

- A run with more than one thread can send several requests to the same API at once. The user must choose a count the API can handle.
- The log is in completion order, so a line's position no longer shows which id came first. Each line still names its id.
- On Resume, ids that completed out of order after the leading run are sent again.
