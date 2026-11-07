---
status: proposed
---

# Run the full suite on macOS and Linux, not only Windows

The CI workflow builds and runs the Core tests on Windows, macOS and Linux, but [ADR 0004](0004-secrets-in-os-storage.md) records that secret storage is verified on Windows only. The macOS Keychain and Linux Secret Service code has never been run against a real store, and parts of the app have Windows-only behaviour. We will run the full Core suite on all three systems, add a secret-storage test on each that writes, reads and removes a value in the real store, and record which behaviours are still Windows-only.

## Decisions within this

- **The same tests on every system:** no platform gets a shorter list, so a failure can't hide behind a skipped test.
- **Real secret storage in tests:** each system's store is used with a throwaway item name, and the test removes what it writes, so no person's secrets are touched.
- **Platform gaps are listed:** anything that still differs by system is written down in the ADR that covers it, so the status is known.
- **A failing platform blocks the release:** a release is not cut while one system's secret test is failing.

## Considered Options

- **Keep Windows-only verification:** no cost, but the other two platforms are shipped on trust.
- **Test macOS and Linux by hand before each release:** possible, but it's the same check done less often.
- **Mock the secret stores:** fast and stable, but a mock can't show that the real store accepts the value.

## Consequences

- CI runners on macOS and Linux may not have a running keyring, so the Linux job must start one, and the setup must be documented.
- Secret tests write to the real stores of the CI machine, so they must run only in CI and never on a person's own machine by default.
- Some tests may need to be marked as platform-specific until the gaps are fixed, and those marks must be reviewed at each release.
