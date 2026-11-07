---
status: accepted
---

# Run a collection once per row of a CSV or JSON data file

Testing often needs one collection run against many inputs, such as a list of users or IDs. Bulk runs (ADR 0015, ADR 0017) send one request per pasted ID, but Ricochet cannot run a whole collection once per row of data, where each row holds several values.

We will let a run take a data file and run the collection once per row, with each row's columns available as variables for that run. Rows run in order, and results are reported per row.

## Considered Options

- Keep bulk runs ID-only: rejected, because a row often needs several values, not one.
- Use a script to loop: rejected, because ADR 0006 keeps checks declarative.

## Consequences

- The data file is read by the app and the CLI, so CI can use it.
- Values from a row stay in memory, like saved values, and are never written to the Workspace.

## What was built

- Core only, in `DataFileParser.cs` and `DataRowRunner.cs`. `CollectionRunner` is unchanged: each row is a separate `RunAsync` call, with the row's values added to `RunOptions.Overrides`, so captures reset per row.
- CSV: the first line is the header, RFC 4180 quoting is honoured, blank and comma-only lines are skipped, and a leading byte-order mark is ignored. JSON: a top-level array of objects. Strings are used as written, numbers and booleans keep their text, `null` is empty, nested values are kept as JSON, and empty objects are skipped.
- Row values come after the given overrides, so a row wins when both name the same Variable. Values are masked in output like other overrides.
- With `StopOnFirstFailure`, a failing row stops the whole data run.
- Not yet wired: the CLI and the App do not read data files yet, so no command-line flag or UI exists for this.
