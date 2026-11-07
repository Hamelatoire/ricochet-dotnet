---
status: proposed
---

# Pin the build's actions and dependencies, and scan them

The CI workflows use actions by version tag, such as `actions/checkout@v5`, and the .NET packages are not pinned by a lock file. A tag can be moved to different code, and a package can ship a known vulnerability without any change to this repository. The release is built from whatever those sources contain on that day. We will pin each GitHub Action to a full commit hash, commit a NuGet lock file, and run a vulnerable-package scan on every pull request, so that what is built is what was reviewed.

## Decisions within this

- **Actions are pinned to commit hashes:** each `uses:` line names the full hash, with the version as a comment, and updates come through a pull request that shows the change.
- **Packages are locked:** the NuGet lock file records the exact version of every package, and the build fails if it doesn't match.
- **A vulnerability scan runs on each pull request:** `dotnet list package --vulnerable` reports known issues, and a high-severity finding fails the check.
- **Updates are visible:** dependency updates are opened as pull requests, so each change is reviewed like any other.

## Considered Options

- **Keep version tags:** simpler to read, but a tag can be moved, which is the risk being removed.
- **Vendor every dependency into the repository:** full control, but it makes the repository large and hard to review.
- **Scan only at release time:** cheaper, but a vulnerability then blocks a release it was never part of.

## Consequences

- Pinned hashes are harder to read, so each one needs its version in a comment, and updating them takes a little more care.
- A scan can fail on a finding that has no fix yet, so the rule must allow a documented exception with an end date.
- The release workflow uses the same locked packages as the check, so a test build and a release build can't differ in their dependencies.
- The `assign-pr.yml` workflow runs on `pull_request_target`, which gives a token write access to pull requests from forks. Its use must be reviewed in the same change, since it's the riskiest workflow in the repository.
