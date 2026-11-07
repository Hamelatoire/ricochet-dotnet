---
status: proposed
---

# Read CLI defaults from a file in the Workspace

A CI command such as `ricochet run --env Staging --xunit results.xml --timeout 30 --insecure` repeats the same options in every pipeline that uses the Workspace. Changing one default means editing every pipeline, and the options are hard to see in the repository. We will let the CLI read its default options from a small file in the Workspace, such as `ricochet.json`, so that the Workspace states how it is meant to run. Options given on the command line still win, and secrets are never read from this file.

## Decisions within this

- **Defaults only, never secrets:** the file can name an Environment, a timeout and a report path, but a Secret value or a token is never accepted in it.
- **Command-line options win:** a flag on the command line overrides the file, so a pipeline can still change a value for one run.
- **Shared, so it is reviewed:** the file is part of the Workspace, so a change to how the Workspace runs shows in a pull request.
- **Unknown keys are an error:** a misspelt option fails the run with a message, rather than being ignored, so a typo can't quietly change the run.

## Considered Options

- **Keep options on the command line:** explicit in each pipeline, but duplicated and hard to change.
- **Environment variables only:** already supported for Variables, but they don't cover the run's options.
- **A separate config file in the CI system:** works, but it moves the run's settings away from the Workspace they describe.

## Consequences

- The Workspace gains another file, which must be covered by the format version in [ADR 0081](0081-version-the-workspace-file-format.md).
- Exit code rules in the CLI are unchanged, so a run configured by the file exits with the same codes as one configured by flags.
- The file must be read before the Workspace is loaded, so an error in it is reported without a partial run.
