---
status: proposed
---

# Built-in functions for generated values

Requests often need a value that differs on every send, such as a unique id or the current time. Without scripts ([ADR 0006](0006-declarative-tests-not-scripts.md)) there is no way to produce one, and an imported `{{$guid}}` stays unresolved ([ADR 0002](0002-import-postman-exports.md)). We will add a fixed set of built-in functions, written inside the existing braces with parentheses, such as `{{uuid.v4()}}`. Because the list is fixed, and functions can't read files, the network or Secrets, a shared Collection still can't run code or send a Secret anywhere.

## First functions

| Function | Result |
|---|---|
| `uuid.v4()` | A random UUID |
| `timestamp.iso8601()` | The current UTC time in ISO 8601 |
| `timestamp.unix()` | Seconds since 1970 |
| `timestamp.unixMillis()` | Milliseconds since 1970 |
| `timestamp.format('yyyy-MM-dd')` | The current UTC time in the given format |
| `random.int(1, 100)` | A whole number in that range, inclusive |

## Decisions within this

- **Parentheses mean a function:** `{{uuid.v4}}` is a Variable with that name and `{{uuid.v4()}}` is a function, so no existing Variable changes meaning.
- **Arguments are plain text and numbers:** a function can't take a Variable or another function as an argument. This can be loosened later, while tightening it would break files.
- **Each use gets its own value:** two uses of `{{uuid.v4()}}` in one send give two ids.
- **A Variable keeps one value per send:** if the Variable `client_id` contains `{{uuid.v4()}}`, every `{{client_id}}` in that send has the same id. This is how to reuse a generated value.
- **Results are final:** a function's result is never searched for more `{{...}}`.
- **Only what is sent can use them:** functions work in the URL, headers, body and auth. Assertions and Captures can't use them, because an expected value that changes on each send can't be checked.
- **An unknown function counts as a missing Variable:** the app warns and still sends, and the CLI fails the Request without sending it.
- **Postman's names keep working:** `{{$guid}}`, `{{$timestamp}}` and `{{$randomInt}}` act as other names for these functions, so files that were already imported start working without being rewritten. Other `$` names stay unresolved and are still listed in the import results.

## Considered Options

- **A plugin system for functions:** would allow anything, including reading files or calling a service, but a shared Collection could then depend on code that runs on every teammate's machine, which [ADR 0006](0006-declarative-tests-not-scripts.md) ruled out.
- **Postman's `{{$name}}` syntax only:** familiar, but it has no clear way to pass arguments, and its names don't group related functions for autocomplete.
- **A separate syntax such as `${uuid()}`:** can't be confused with a Variable, but `${...}` is common in real request bodies, so existing text would change meaning.

## Consequences

- This changes the import scope in [ADR 0002](0002-import-postman-exports.md) if accepted, and that ADR needs updating.
- Adding a function is easy, but removing or renaming one breaks shared files, so each name is a long-term commitment.
- History records the value that was sent, so an entry shows the id actually used rather than the function.
- The same Request produces different output each time, so a Run can't be compared byte for byte unless its generated values are ignored.
- A text result is inserted as written, like a Variable's value, so `timestamp.format` with a `"` in its format can break a JSON body.
