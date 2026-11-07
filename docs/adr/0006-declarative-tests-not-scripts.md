---
status: accepted (built)
---

# Declarative assertions instead of running JavaScript

People moving from Postman need to verify responses and run those checks in CI. We chose declarative assertions stored as data on each Request, such as "status equals 200" or "JSON `items[0].id` exists", rather than running scripts, because they keep the app small and fast, can be read and edited in a form, and produce clean diffs in git. Postman scripts stay preserved but unrun, as [ADR 0002](0002-import-postman-exports.md) describes.

## Considered Options

- **Embed a JavaScript engine with a `pm` API:** imported Postman tests would run unchanged, which is the main thing given up here. But it means a large compatibility surface to keep matching, a bigger binary, an engine that must work under Native AOT, and running untrusted code from shared collections with access to Secrets.
- **Automatically translate Postman scripts into assertions:** only simple scripts could be translated, and a partly translated test suite looks more trustworthy than it is.

## Decisions within this

- **CI must not pass by accident:** the CLI exits with an error if a file can't be read, if nothing matched the selection, or if a Request needs a Variable with no value. A Request in that last case is not sent, so a literal `{{token}}` never reaches a server.
- **Scripts that didn't run are always mentioned:** a green run must say how many Requests have scripts that were skipped, because those may be the real tests.
- **Secret values are masked in CLI output and reports:** CI logs are often widely readable.
- **Assertions are stored on the Request, and omitted from the file when there are none:** this adds nothing to the diff of a Request that has no tests.
- **Chaining is declarative too:** a Request can capture a value from its response into a Variable, such as a login token, for later Requests in the same Collection.
- **Captured values are never written to a file:** they are often tokens, so they live in memory for the run or app session, and are masked in reports. Only the rule for capturing them is stored. They override Collection, Environment and Global Variables, and values given for a CLI run override them.
- **A capture that finds nothing fails its Request:** later Requests depending on it then fail without being sent, which points at the real cause.

## Consequences

- Existing Postman test suites must be rewritten as assertions to count as tests.
- Captures depend on order, so running only a folder fails if the Request that captures its token is outside it. In the app, the login must be sent again after a restart.
- All captured values are masked in CLI output, including harmless ones such as an id, which can make a failure message less helpful.
- The app warns about a missing Variable but still sends the Request, because exploring by hand differs from CI, where a missing value must stop the Request.
- Logic that assertions can't express, such as loops or computed values, isn't possible. If that proves a real need, this decision should be revisited rather than worked around.
