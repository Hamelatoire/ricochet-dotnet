---
status: proposed
---

# Prompt for a variable when a request is sent

Some values change on every send, such as a one-time code or the ID of a record to fetch. Today every value must come from an environment, a collection, the globals or a saved response, so these values are edited by hand before each send.

A variable can be marked as prompt. When a request that uses it is sent, the app asks for the value first, and the answer is used for that send only.

## Considered Options

- Always ask for every value: rejected, because most values are fixed.
- Store the answer like an environment value: rejected, because one-time values would then persist.

## Consequences

- In CI, a prompt variable with no value fails the request without sending it, the same as an unset variable today.
- Prompt answers stay out of History, like secrets.
