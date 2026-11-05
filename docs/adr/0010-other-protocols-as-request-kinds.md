---
status: proposed
---

# Add other protocols as kinds of Request, starting with those over HTTP

People testing an API often need more than plain HTTP, but each new protocol adds to the binary and to what has to be tested, and speed is the reason for Ricochet to exist. We will add protocols one at a time, in order of how little they change: GraphQL and server-sent events first because they are still one HTTP Request, then WebSocket, then gRPC. They all live in the same Collections and use the same Variables and Secrets, with a `kind` field in the Request's file that is omitted for HTTP, so existing files and their diffs don't change.

## Order and reasons

| Protocol | What it changes | Decision |
|---|---|---|
| GraphQL | Only how the body is edited: a query and variables, sent as a POST | First |
| Server-sent events | The response arrives over time, so the response view must show a stream | Second |
| WebSocket | A connection with messages both ways, not one request and one response | Third, and needs its own design |
| gRPC | Needs `.proto` files or server reflection, plus HTTP/2 and message encoding | Last, only after a prototype under Native AOT |

## Decisions within this

- **One tree for every kind:** a WebSocket connection sits beside the HTTP Requests for the same API, rather than in a separate mode or tab of the app.
- **No hidden stop in a Run:** a stream may never end, so a streaming Request in a Run needs a stated limit, and hitting that limit without its Assertions passing fails it.
- **A new protocol must earn its place:** one that noticeably increases startup time or binary size needs a new decision, even if it's already on this list.

## Considered Options

- **HTTP only:** the smallest and simplest app, and a clear position. But GraphQL costs very little, and people with one WebSocket or gRPC service would keep Postman installed for it.
- **A separate tool or file type for each protocol:** keeps the HTTP code untouched, but splits Collections, Variables and Secrets that describe the same API.
- **Plugins for protocols:** keeps the core small, but loading code at run time conflicts with Native AOT and with [ADR 0006](0006-declarative-tests-not-scripts.md).

## Consequences

- The glossary defines a Request as an HTTP call. That definition has to change when WebSocket arrives, and **Run** and **Assertion** need rethinking for messages.
- Workspace files have no format version today. An older build would read a WebSocket file as a broken HTTP Request, and saving could lose its fields, which breaks the promise in [ADR 0003](0003-git-friendly-workspace-files.md). So the first new `kind` needs a way for older builds to report the file and leave it alone, and that should ship before any file uses it.
- The response view needs to handle a stream, which also affects the size limit for responses in [ADR 0007](0007-personal-settings-page.md).
- Postman exports can contain these kinds too, so [ADR 0002](0002-import-postman-exports.md) needs revisiting as each one arrives.
