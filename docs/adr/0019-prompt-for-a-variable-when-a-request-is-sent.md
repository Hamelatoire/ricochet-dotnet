---
status: accepted
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

## Built

- `KeyValue` gained an `IsPrompt` flag, saved only when set, like `IsSecret`. A Variable is marked with a Prompt toggle in the Variables and Globals editors.
- `VariableResolver.FindPromptVariables` (Core) finds the prompt Variables a Request uses, directly or through another Variable. Only the Variable that wins its name counts, so a plain value in a higher scope skips the prompt.
- `VariableResolver.Resolve` takes the answers for one send. An empty answer leaves the name unresolved, and History keeps the Variable as written.
- The App shows one prompt before a send, pre-filled with the stored value. Cancelling sends nothing. CI and Runs don't prompt, so a prompt Variable with no stored value fails there, as an unset Variable does.
