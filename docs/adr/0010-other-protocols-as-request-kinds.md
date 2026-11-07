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

## Open questions

Found while reading the code in this worktree (`origin/main`, commit 277aa41). None of these changes the decision above.

- **Streaming doesn't exist yet.** `RequestSender.SendThroughAsync` reads the whole body through `ReadBodyAsync` before it returns an `ApiResponse`, so an event stream that never ends returns nothing until the timeout (120 seconds by default) and then fails.
- **The stream limit has no existing model.** A timeout throws `RequestFailedException`, while the body cap returns `Truncated` as a success, so the ADR should say whether reaching a stream limit passes or fails, and whether the limit counts time, messages or bytes.
- **Where `kind` goes is not settled.** The ADR says "the Request's file", but that file is `RequestFile`, which nests an `ApiRequest`; the choice decides the file diff and the shape of the tree.
- **The tree is typed to HTTP.** `CollectionItem.Request` is `ApiRequest?`, so "one tree for every kind" means changing that type or adding a sibling field, and the ADR does not pick one.
- **The older-build guard also applies to GraphQL.** The Consequences ask for it before the first file with a new `kind`, and GraphQL is that first kind, yet the Order section presents it as a change that leaves files alone. Workspace files still have no version field, so the guard needs a marker design (a new file name, a version, or a skip rule in `WorkspaceFolder.Load`).
- **GraphQL storage is unstated.** `ApiRequest.Body` is one string, and `RequestSender.BuildMessage` guesses the Content-Type from its first character, so the ADR should say whether the query and variables are a JSON envelope in `Body` or new fields, and how the body editor, `VariableResolver` and `CurlGenerator` treat them.
- **Only the importer is named.** `ApiRequest` is also read or written by `InsomniaImporter` ([ADR 0033](0033-import-insomnia-exports.md)), `PostmanExporter`, `CollectionExport`, `OpenApi`, `ResponseExamples` and `CollectionDocumentation`, and the ADR does not say which of them must handle a new kind and which should report it as skipped.
- **The glossary change is a prerequisite.** `GLOSSARY.md` defines a Request as "a saved definition of an HTTP call", and `Assertions` only read an `ApiResponse`, so Run and Assertion need decisions before any WebSocket or stream work starts, not only a rewrite of the glossary entry.
- **Native AOT is only checked for gRPC.** The ADR asks for an AOT prototype only for gRPC, but any new saved type must be registered on `WorkspaceJsonContext`, and a `kind` that is missing on HTTP files must still load with source generation. This should be checked with `dotnet publish -r win-x64` for GraphQL too, because it adds a saved shape.

**First step, estimated:** the GraphQL kind with the older-build guard would touch about 10 to 12 source files, mostly in `Ricochet.Core` (`Models.cs`, `WorkspaceFolder.cs`, `WorkspaceStore.cs`, `RequestSender.cs`, `CurlGenerator.cs`, `PostmanImporter.cs`, `InsomniaImporter.cs`), plus `RequestTabViewModel.cs`, `MainViewModel.cs`, the request view in `MainWindow.axaml`, and Core tests. The guard alone is about 3 files.
