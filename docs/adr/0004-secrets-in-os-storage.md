---
status: accepted (built; verified on Windows only)
---

# Keep secrets in the operating system's secure storage

Tokens and passwords are stored in plain text today, and once Workspaces can live in git ([ADR 0003](0003-git-friendly-workspace-files.md)) that would become a way to leak them publicly. We will let a Variable be marked as a Secret and keep its value in secure storage provided by the OS: DPAPI on Windows, Keychain on macOS and Secret Service on Linux. Workspace files hold only the Variable's name and the fact that it is a Secret. We chose this because the OS already ties these to the user's login, so there is no separate password to manage.

## What counts as a Secret

- Variables the user marks as Secret, and those Postman exports mark with `type: secret`.
- Values typed directly into bearer, basic and API key auth fields.

An `Authorization` header or a token pasted into a URL can't be reliably recognised in general, so the app will warn about likely cases and recommend using a Secret Variable instead. It will not try to guarantee detection.

## One protected item

All Secrets are stored together as one protected item rather than one item each. Each OS then only has to read and write a single value, and a Secret that was removed disappears at the next save without any cleanup. If secure storage can't be read, the app must not write over it: it keeps Secrets in memory for the session and warns the user.

## History

History can contain literal credentials, so it stays in personal storage, keeps `{{placeholders}}` unresolved, and masks the values of sensitive headers such as `Authorization` and `Cookie`.

## Considered Options

- **Encrypted file with a master password:** works the same on every OS, but adds a password to enter and to lose, and makes us responsible for our own cryptography.
- **Separate git-ignored secrets file:** simple and needs no OS integration, but it remains plain text on disk, and a single wrong ignore rule publishes everything.
- **Plain text with a warning:** what happens today, and not acceptable once files are shared.

## Consequences

- Secret values don't travel with the repository, so each person on a team enters their own. The app must clearly show which Secrets have no value on this machine.
- Secrets are lost if the OS profile or keychain is reset, because they can't be exported. That is a deliberate trade-off.
- Each OS needs its own Native AOT-compatible code behind one interface, and this must be tested on all three.
- Linux machines without a running Secret Service, such as minimal desktops, can't store Secrets. The app will keep them in memory for the session and say so, rather than quietly falling back to plain text.
- Existing plain-text values in auth fields need migrating, and old copies remain in any backup such as the original `PostmanLight` folder, so the app should tell the user about them.
