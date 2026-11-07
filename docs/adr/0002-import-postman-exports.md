---
status: accepted (built)
---

# Import Postman exports into our own model

People need to be able to move over from Postman without rebuilding their work, so the app imports exported Postman files: Collections (schema v2.0 and v2.1), Environments and Globals. Imports are converted once into our own model rather than storing Postman's format, so our storage is not tied to Postman's schema.

## Variable precedence

We follow Postman's order, so imported requests resolve to the same values: an Environment Variable overrides a Collection Variable, which overrides a Global Variable. A disabled Variable never hides one in a lower scope.

## Scope

- **Imported:** folder structure and order, method, URL with path variables, headers, Collection Variables, and raw, urlencoded, GraphQL and text form-data bodies.
- **Auth:** bearer, basic and API key are imported. Inheritance is resolved during import, so each Request holds the auth it will use and later edits to one Request cannot change another. Other types, including OAuth 2, are reported and the Request is left with no auth.
- **Scripts are kept but not run:** pre-request and test scripts would require an embedded JavaScript engine and a Postman-compatible `pm` API, which is a large commitment for a tool whose goal is to be fast and small. They stay in the Workspace, so running them later would not need a re-import.
- **Dynamic variables:** `{{$guid}}`, `{{$timestamp}}` and `{{$randomInt}}` resolve to built-in functions ([ADR 0008](0008-built-in-functions-for-generated-values.md)). Other dynamic variables stay as written and are listed in the import report.
- **Not carried over:** file uploads, disabled form fields and query parameters, saved example responses and descriptions.
- **Files only:** no Postman account sign-in or API sync, so the app needs no credentials and no network access to import.
- **Not an import:** repeatable updates from an OpenAPI URL are a Spec link, described in [ADR 0014](0014-collections-from-openapi-url.md). This ADR's one-time import is unchanged.

## Reporting and name clashes

An import never silently drops something it does not understand: it converts it or lists it in the import report. Importing never overwrites a Collection or Environment. A clash gets a numbered name such as "Shop API (2)". Global Variables merge by name, and the report lists any values replaced.

## Consequences

- Collections that rely on scripts, for example to fetch an auth token, need those values set by hand. The app warns about this on the affected Requests, and names any placeholder left without a value when sending.
- Exports can contain real secrets. Variables Postman marks as secret, and values typed into auth fields, go to secure storage ([ADR 0004](0004-secrets-in-os-storage.md)). Other Variables stay in the Workspace file even if they hold something sensitive, so the user must mark those as Secret.
