---
status: proposed
---

# Import Insomnia exports

Users with an existing Insomnia collection must recreate their requests by hand. Ricochet imports Postman and OpenAPI only.

Ricochet will import Insomnia exports under the same rules as Postman imports (ADR 0002): convert what it can, report what it can't, and never overwrite an existing collection.

## Considered Options

- Import only through OpenAPI: loses names, folders and environments.

## Consequences

- A third format adds parser code and test fixtures.
- YAML needs a parser that works under Native AOT, which is a decision in its own right (see ADR 0014).
