---
status: proposed
---

# Import HAR files exported from a browser

A common way to reproduce a bug is to record the requests a web page made, which browsers export as a HAR file. Turning those requests into Requests by hand is slow and error-prone. We will let a HAR file be imported as a Collection, with one Request per recorded entry, using the same import rules as the Postman and Insomnia importers: a one-time conversion that reports what it could not carry over.

## Decisions within this

- **One Request per entry:** method, URL, headers and body come across. Each entry keeps its response as a saved example, using [ADR 0023](0023-save-example-responses-on-a-request.md).
- **Cookies and auth headers become Secrets:** values that look like credentials are stored as Secrets, not in the Workspace file, following [ADR 0004](0004-secrets-in-os-storage.md).
- **Preview first:** the import shows what will be written and what was skipped, as the importer preview in ADR 0073 describes.
- **Non-HTTP entries are skipped and named:** WebSocket frames and other entries that are not requests are reported, not silently dropped.

## Considered Options

- **Ask people to copy requests by hand:** no code, but it's the slow path this removes.
- **Replay the HAR file directly:** it would send traffic the person didn't write in this app, and HAR files are often recorded with cookies that have expired.
- **Support only one browser's export:** possible, but HAR is a standard format, so one importer covers all of them.

## Consequences

- HAR files can contain sensitive data, so the importer must mask credentials in any report it writes, consistent with ADR 0076.
- Large recordings need the same progress feedback as large imports.
- The importer must not run the imported requests, following [ADR 0002](0002-import-postman-exports.md)'s rule that imports never run scripts or send traffic.
