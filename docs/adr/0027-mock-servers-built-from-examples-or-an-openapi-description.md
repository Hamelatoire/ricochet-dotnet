---
status: accepted (built in Core and the app; saved examples not built yet)
---

# Mock servers built from examples or an OpenAPI description

Developers often need an API that is not built yet, or one they cannot reach. Ricochet can only send requests to a real server, so there is no way to answer them locally.

Ricochet may run a local mock server that answers requests from saved examples (ADR 0023) or an imported OpenAPI description (ADR 0014). It listens only on localhost, and only while the user starts it.

## Considered Options

- Use a separate tool: keeps Ricochet small, but splits the work between two apps.
- Listen on all interfaces: rejected, because mocks would be reachable from the network.

## Consequences

- This is a large feature, so it needs its own design before any code.
- Opening a listening port is a security decision and needs a clear warning.

## What was built

- Core (`MockServer.cs`): routes from an OpenAPI 3 JSON description (first 2xx response; its example, else a sample built from the schema), and from `MockExample` values. Matching ignores the query string, prefers literal segments over parameters, and answers unknown paths with 404 and wrong methods with 405 (JSON bodies). The server listens on `127.0.0.1` only, between Start and Stop, and logs the path and status only.
- App: a "Mock server" button and dialog that load a description from a file, set a port (1024 to 65535), start and stop the server, and show a request log.
- Not yet: saved examples. They depend on [ADR 0023](0023-save-example-responses-on-a-request.md), which is not built in this change. `MockRouteBuilder.FromExamples` is ready for it; no example storage was added here.
