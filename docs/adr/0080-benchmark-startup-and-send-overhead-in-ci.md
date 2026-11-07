---
status: proposed
---

# Benchmark startup time and send overhead in CI

Speed is the reason the app exists, and [ADR 0001](0001-csharp-avalonia-native-aot.md) chose Native AOT for it. Nothing in the build measures speed, so a change that adds a second to startup, or a few milliseconds to every send, merges without anyone noticing. We will measure cold startup time and the overhead the app adds to a send, against a local test server, in CI, and fail a pull request that makes either one slower than a set limit.

## Decisions within this

- **Measure what the app adds:** the send benchmark times the work the app does around the HTTP call, against a local server that replies at once, so network variation doesn't hide the app's own cost.
- **Cold start is measured on a clean run:** each start is a new process, so caching from a previous run doesn't flatter the numbers.
- **A limit with a margin:** a change fails only past a set allowance above the recorded baseline, so normal noise doesn't block work.
- **Results are kept:** each run's numbers are stored with the commit, so a slow trend is visible over time.

## Considered Options

- **Measure by hand before each release:** cheap, but a release is the wrong time to find a regression.
- **Measure only on a dedicated machine:** steadier numbers, but it's an extra machine to keep running for a small project.
- **Measure wall-clock time of the test suite:** easy to get, but it doesn't measure the app's startup or send path.

## Consequences

- CI runners are shared and noisy, so the margin must be wide enough to avoid false failures, and the limit is revisited when that's known.
- The benchmark needs the Native AOT publish, so it adds build time to each pull request.
- The local test server used for the benchmark must not reach the network, consistent with [ADR 0005](0005-local-first.md).
