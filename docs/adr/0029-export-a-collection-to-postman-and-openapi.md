---
status: proposed
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
