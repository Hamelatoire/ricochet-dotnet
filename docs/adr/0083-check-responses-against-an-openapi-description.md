---
status: proposed
---

# Check responses against an OpenAPI description

The app can create a Collection from an OpenAPI description ([ADR 0014](0014-collections-from-openapi-url.md)), but after that it doesn't use the description again. A response that doesn't match the documented shape, such as a field that was renamed or a status code the spec doesn't list, passes unless someone writes an Assertion for it. We will add a Collection-level check that compares each response with the schema in the linked description, and reports any status code or field that the description doesn't allow.

## Decisions within this

- **Uses the description already linked:** the check reads the Spec link the Collection already has, so no new setup is needed.
- **Reports, as an Assertion does:** a mismatch is reported as a failed check in the run, with the field or code that differs, using the same result shape as other Assertions.
- **Off by default:** a Collection opts in, so an existing run's results don't change when the feature arrives.
- **Local description only:** the check reads the description that was already fetched, and does not fetch it again during a run, consistent with [ADR 0005](0005-local-first.md).

## Considered Options

- **Write the Assertions by hand:** precise, but it's the work the description already contains.
- **Check only status codes:** simpler, but most drift is in the body.
- **Use a separate contract-testing tool:** capable, but it's a second tool for the same job, and the app is meant to cover it.

## Consequences

- OpenAPI schemas have many features, and not all can be checked, so the check must say what it skipped, as the importers do.
- A description that is out of date will fail runs, which is the intended signal, but the report must make clear the fault may be in the description.
- The check adds work per response, so it must be measured with the benchmark in [ADR 0080](0080-benchmark-startup-and-send-overhead-in-ci.md).
