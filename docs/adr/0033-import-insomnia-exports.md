---
status: accepted (built in Core only; not yet wired into the app or CLI, and YAML exports are not read)
---

# Import Insomnia exports

Users with an existing Insomnia collection must recreate their requests by hand. Ricochet imports Postman and OpenAPI only.

Ricochet will import Insomnia exports under the same rules as Postman imports (ADR 0002): convert what it can, report what it can't, and never overwrite an existing collection.

## Considered Options

- Import only through OpenAPI: loses names, folders and environments.

## Consequences

- A third format adds parser code and test fixtures.
- YAML needs a parser that works under Native AOT, which is a decision in its own right (see ADR 0014).

## What was built

- `InsomniaImporter` in `src/Ricochet.Core/InsomniaImporter.cs` reads Insomnia **format 4 JSON** exports. YAML exports are not read yet, so the YAML question above is still open. Other format numbers are refused with a message to export again.
- Each workspace in the export becomes a Collection. Its base environment becomes Collection Variables, and each sub-environment becomes an Environment, so the usual precedence applies.
- Folders and requests keep their order. Headers and auth on a folder apply to the requests inside it unless the request sets its own. A request set to inherit, or with no auth type, takes what its folder or workspace has. `none` means no auth.
- Bearer, basic and API key auth are imported. Other types, such as OAuth 2, digest and NTLM, are reported and the Request is left with no auth.
- Bodies: text bodies keep their MIME type as Content-Type, and URL-encoded and multipart forms become the same text forms the Postman importer writes. File uploads are reported.
- Placeholders: `{{ name }}` and `{{ _.name }}` become `{{name}}`. `{% uuid 'v4' %}` and `{% timestamp 'unix' %}` map to the built-in functions (ADR 0008). Other Insomnia tags are left as written and listed in the report.
- Reported and not imported: descriptions, disabled query parameters and form fields, cookies, and resources such as gRPC, WebSocket and unit tests.
- **Not yet wired into the app.** The file picker and the import flow still accept only Postman and OpenAPI files. Calling `InsomniaImporter.Import` is the only way in until that follow-up lands.
