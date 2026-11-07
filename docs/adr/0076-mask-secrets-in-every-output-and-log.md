---
status: proposed
---

# Mask Secrets in every output, log and export

[ADR 0004](0004-secrets-in-os-storage.md) keeps Secret values out of the Workspace, and the CLI masks them in its own output. Secrets can still reach other places: the bulk run log, the curl text, the HTML documentation, the Postman and OpenAPI exports, and the ZIP and Environment exports. Each of these was designed on its own, so a Secret that appears in one of them may not be masked there. We will treat every output that leaves the process, including files on disk, as a place where Secret values must be masked, and check each one against a single list of Secret values.

## Decisions within this

- **One list, used everywhere:** the set of Secret values is built once for a run or an export, and every writer masks against that list.
- **Masked is the default:** an export that includes Secrets must say so and require the person to choose it. Without that choice, Secret values are replaced with a placeholder.
- **The bulk run log is covered:** `bulk-log.txt` in the personal folder is written line by line, so each line is masked before it's written.
- **Tests check each writer:** every output has a test that puts a known Secret into a Request and checks that the value is absent from the output.

## Considered Options

- **Mask only the screen:** what exists today for most views, but files and exports keep the value.
- **Warn in the docs and leave the rest to the person:** cheap, but a shared export that contains a token is a real leak.
- **Never allow Secrets in exports at all:** safest, but it removes the ability to move a workspace between two machines of the same person, which [ADR 0036](0036-export-and-import-environments-without-secrets.md) already handles in a different way.

## Consequences

- An export that contains a masked Secret can't be imported as a working copy, which must be explained where the export is made.
- The list of Secret values must be kept in memory only for the time of the run, consistent with the rule that captured values are never written to a file.
- Each new output format needs a masking test before it ships, so this becomes part of the checklist for new exports.
