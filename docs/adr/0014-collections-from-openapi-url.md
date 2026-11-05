---
status: proposed
---

# Create or update a Collection from an OpenAPI URL

Many APIs publish an OpenAPI (Swagger) description, and writing a Request for each operation by hand is the slow part of trying a new API. We will let the user paste the URL of a description and create a Collection with a Request per operation, grouped into Folders by tag. Running the same URL again updates that Collection: new operations are added and changed ones are updated, while the user's own additions are kept.

## Decisions within this

- **Fetch only when asked:** the user gives the URL and presses the button. [ADR 0005](0005-local-first.md) allows this, because it's a request the user chose. Nothing refreshes on a timer, and the URL is shown before it's fetched. It uses the same TLS settings as any Request.
- **Auth becomes Variables, never values:** a security scheme maps to Bearer, Basic or API key auth that reads a `{{variable}}`. Values in the description, such as example tokens, are never copied, so they can't end up in a shared file.
- **The server becomes a Variable:** the first server URL is stored as a Collection Variable, such as `{{baseUrl}}`, so each Environment can point at its own host.
- **Updates change only what the description owns:** the name, method, URL and documented parameters are updated from the description. Headers, bodies, Assertions and Captures the user added are left alone.
- **Operations keep their ids:** an operation is matched to an existing Request by its `operationId`, or by method and path when it has none. The id stays the same, so [ADR 0003](0003-git-friendly-workspace-files.md)'s file names and diffs stay stable.
- **Removed operations are kept and reported:** an update never deletes a Request. One that's no longer in the description is listed for the user to delete, since deleting is destructive.
- **Preview before applying:** the user sees what will be added, changed, kept and reported, and applies it with a confirmation, as copying into a folder already does.

## Considered Options

- **Import a downloaded file once:** this is how Postman exports already work, and it's simpler, but descriptions change, and people re-import by hand and lose their edits.
- **Keep the Collection in sync automatically:** rejected, because it means network traffic the user didn't ask for, which [ADR 0005](0005-local-first.md) rules out.
- **Generate code or a typed client:** a different product, and it doesn't help someone who wants to send requests.

## Consequences

- This is a repeatable sync, which the glossary currently rules out: **Import** is defined as a one-time conversion, with "Sync" listed under _Avoid_, and [ADR 0002](0002-import-postman-exports.md) describes import the same way. If this is accepted, the glossary needs a separate term for it, such as "Spec link", and that ADR needs a note.
- Descriptions can reference other documents by URL. Resolving those is network traffic, so only references inside the same document are followed, and the rest are reported.
- A description is data from outside the app. Its text, such as descriptions and examples, is shown as text and never run.
- Request bodies can be filled from an example in the description, but schemas describe shape rather than values, so a body created this way may still need editing.
- Starting with OpenAPI 3 in JSON keeps the work small. YAML and Swagger 2.0 would each need a decision about a parser under Native AOT, so they are not part of this ADR.
