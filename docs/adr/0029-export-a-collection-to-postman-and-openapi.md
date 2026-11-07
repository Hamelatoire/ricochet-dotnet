---
status: accepted (built in Core; no App or CLI command yet)
---

# Export a collection to Postman and OpenAPI

Ricochet imports Postman (ADR 0002) and OpenAPI (ADR 0014) but cannot export. Work done in Ricochet therefore cannot be handed to a team that uses those tools.

Ricochet will export a collection to a Postman v2.1 file and to OpenAPI 3 JSON. Anything the target format can't hold is listed in an export report, as imports are (ADR 0002). Secrets are never exported.

## Considered Options

- Export only Postman: OpenAPI is the other common target.
- Silent loss: rejected, because every dropped item must be reported.

## Consequences

- Exports will not round-trip exactly, which the report must say.
- Requires the same secrets rule as ADR 0004.

## What was built

- `PostmanExporter.Export` writes a Postman v2.1 collection, which `PostmanImporter` reads back to the same Requests, Folders, headers, bodies, auth, Variables and scripts. A test checks this round trip.
- `OpenApiExporter.Export` writes an OpenAPI 3.0 JSON document: one operation per Request, grouped by Folder, with path and query parameters, a request body example, and a security scheme for bearer, basic and API key auth. The OpenAPI importer reads it back.
- Both return an `ExportResult`: the JSON, plus a report of everything left out, grouped as imports are.
- Secrets: Secret Variables are left out and their references stay as `{{name}}`. A typed-in bearer token, basic password, API key or sensitive header becomes a `{{placeholder}}` in Postman, and is omitted from OpenAPI, which keeps only the sign-in scheme. Any other text that holds a secret's value has it replaced with its placeholder.
- Not exported, and reported: assertions and captures, scripts in OpenAPI, Collection Variables in OpenAPI (their values are not written), methods OpenAPI does not have, and a second operation with the same method and path.
- Not covered: a credential inside a URL or body that is not a known Secret. This follows ADR 0004's best-effort detection.
- Still to do: an export command in the App and the CLI.
