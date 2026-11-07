---
status: proposed
---

# Start from a sample Workspace on first run

A new install opens to an empty sidebar, and the first useful action is to create a Collection, a Request and an Environment. Someone who has never used a client like this has to work out the model before they see the app do anything. We will offer, on first run, a sample Workspace that calls a public test API with a few Requests, an Environment and one Assertion. It is created only when the person chooses it, and it can be removed like any other Collection.

## Decisions within this

- **Offered, never forced:** the first run asks whether to add the sample, with a choice to start empty. Nothing is created without the person's answer.
- **The sample is a normal Workspace:** it is plain files in a folder the person chooses, so it can be opened, edited and deleted like any other, and it shows how a real Workspace looks on disk.
- **It calls a public test API with no account:** the requests need no key or sign-in, so it works offline apart from the calls themselves and needs no Secret.
- **It is marked as a sample:** its Collection name says so, so it isn't mistaken for work.

## Considered Options

- **An empty app with a help page:** no code, but reading a page is slower than trying a request.
- **A guided tour with overlays:** clearer, but it blocks the window and is awkward to skip.
- **Ship the sample inside the app and load it on demand:** the same result, but the files in the app folder are then harder to find and change.

## Consequences

- The sample must stay valid as the public test API changes, so it needs a check, which the Core tests can run.
- The app makes network calls when the sample is used, which is what the person asked for, so it does not break [ADR 0005](0005-local-first.md).
- Screenshots for this change must use the throwaway data set in `RICOCHET_DATA`, as the repository requires.
