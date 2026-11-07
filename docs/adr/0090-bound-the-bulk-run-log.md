---
status: proposed
---

# Bound the bulk run log

Every bulk run appends a line per request to `bulk-log.txt` in the personal folder, and nothing ever removes a line. A person who runs a large data file every day adds thousands of lines a week, the file grows without limit, and the log view that reads it gets slower as it grows. The log is personal and useful only for the run in front of the person, so keeping every run forever has little value. We will keep the log for the last set number of runs, and remove the oldest runs when that number is passed, so the file stays a bounded size.

## Decisions within this

- **Kept by run, not by line:** a run's lines are removed together, so no run is left half-logged.
- **A fixed number of recent runs:** the last ten runs are kept by default, and the person can change the number on the settings page from [ADR 0007](0007-personal-settings-page.md).
- **The current run is never removed:** a run in progress keeps its lines until it finishes, so the log view shows the whole run.
- **Clearing still works:** the existing Clear log action removes everything at once, as it does today.

## Considered Options

- **Keep every line forever:** no work, but the file grows without limit and slows the log view.
- **Delete the log when the app closes:** keeps the file small, but a person who wants to check yesterday's run loses it.
- **Move old runs to an archive file:** keeps them, but it is a second file to manage, and the person rarely opens it.

## Consequences

- Each line must carry its run identifier, so a run's lines can be found and removed together. Existing lines without one are treated as a single old run.
- Trimming happens when a run starts, so a long run is never cut short by a trim.
- The log holds request lines from [ADR 0015](0015-bulk-runs-with-resume.md) runs, so resuming a run must still find its earlier lines, which the trim must not remove for an unfinished run.
- Log lines must already be masked for Secrets, as ADR 0076 requires; trimming does not change what is written.
