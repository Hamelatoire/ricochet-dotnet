---
status: accepted (built)
---

# Translate simple Postman test checks on import

Postman collections imported into Ricochet keep their tests as scripts, which Ricochet does not run (ADR 0002, ADR 0006). Those tests do nothing after import.

On import, Ricochet converts the simple checks it recognises into assertions (ADR 0006). Anything it doesn't recognise stays as a kept script, reported in the import report.

## Considered Options

- Keep scripts only: what happens today, and tests stop working.
- Run the scripts: rejected, because that needs a JavaScript engine (ADR 0006).

## Built

- `PostmanTestTranslator` (Core) turns a Request's own `test` script into assertions. Folder, Collection and pre-request scripts are never translated.
- A script is translated only if every line is a recognised check. Otherwise none of it is, so a partly understood test never looks converted. The import report then names the first line it could not translate.
- Recognised checks: `pm.response.to.have.status(N)`, `pm.expect(pm.response.code).to.eql(N)` and `pm.expect(pm.response.code === N).to.be.true`; `pm.expect(pm.response.responseTime).to.be.below(N)`; `pm.response.to.have.header(name[, value])`; `pm.expect(pm.response.json().path).to.eql(literal)`, also through `const x = pm.response.json()`. Each may sit inside `pm.test(name, function () { ... });` or an arrow function.
- JSON values are compared as text, so a JSON string `"1"` also matches the number `1`, where Postman's `eql` does not. Translated checks are therefore slightly looser for that case.
- Checks whose meaning differs here are left out: `.to.exist` (a JSON `null` passes Ricochet's exists check), body `include` (Ricochet compares the formatted body), and `oneOf`.

## Consequences

- Each recognised pattern needs a test, so the list of supported checks must be published: it is the list above, and the tests in `PostmanTestTranslatorTests`.
- A partial translation can give false confidence, so the report must list what was not converted.
