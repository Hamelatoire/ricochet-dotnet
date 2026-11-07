---
status: proposed
---

# Prepare the interface text for translation

Every label, message and tooltip in the app is written directly into the views and the view models, in English. Adding a second language would mean finding and changing every one of them. Users who work in other languages can't easily use the app, and the text is spread too widely for a translator to see all of it at once. We will move the text into resource files, keyed by name, so that a translation is a new file and not a change to the code. The first step is English only.

## Decisions within this

- **Text lives in resource files:** each string has a stable key and an English value, and the views refer to the key, not to the words.
- **English is the only language for now:** the first change moves every string but adds no translation, so nothing visible changes.
- **Keys describe the purpose:** a key names what the text does, such as `Send.Button`, not the words, so a wording change doesn't change the key.
- **Missing translations fall back to English:** a partly translated app still works, rather than showing blanks.

## Considered Options

- **Leave the text in the views:** no work, but adding a language later costs much more.
- **Translate in one large pass later:** avoids moving text twice, but it's a large change to the whole app at once.
- **Machine translation at run time:** quick to try, but it needs a network service, which [ADR 0005](0005-local-first.md) rules out for the app.

## Consequences

- Every string moves in the same change, so a missed one will show English in a translated build. A check that finds strings left in views is needed.
- Layouts must allow for longer text, since many languages are wider than English, and this affects the density work in [ADR 0042](0042-denser-layout-and-larger-controls.md).
- Only the text is moved. Right-to-left layout and date and number formats are left for a later decision.
