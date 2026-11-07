---
status: proposed
---

# Sign the Windows installer and publish checksums

The Windows installer is built with Inno Setup in the release workflow and is not signed. When a person first ran the app, Windows Defender reported it as untrusted, which is the sign that a downloaded program is not from a known publisher. A tool that people download and then trust with their credentials should prove where it came from. We will sign the installer and the portable executable with a code-signing certificate in the release workflow, and publish a SHA-256 checksum next to every release file so a download can be checked.

## Decisions within this

- **Sign every release binary:** the installer and the published `.exe` are signed in the release workflow, not by hand, so an unsigned build can't be released by accident.
- **The certificate is held in the repository's secret store:** the key never appears in a file or a log, and only the release job can read it.
- **A checksum file is published with each release:** `SHA256SUMS` lists each file, so someone who downloads from a mirror can check it.
- **Verify before publishing:** the release job checks the signature after signing and fails if it is missing, so an unsigned file never reaches the release page.

## Considered Options

- **Leave it unsigned and explain the warning:** free, but it trains people to click through security warnings, which is the habit attackers rely on.
- **Publish checksums only:** useful for a careful reader, but a checksum on the same page as the file proves little if the page is compromised.
- **Use a store that signs for you:** removes the certificate to manage, but it adds a store and its rules, which a small project may not want yet.

## Consequences

- A code-signing certificate has a yearly cost and must be renewed, and an expired one stops releases until it is replaced.
- Signing uses a secret, so pull requests from forks must not reach it, which the release workflow must enforce.
- Windows reputation builds up over time, so a new certificate may still show a warning for a while. This is a known limit, and the checksum is the fallback.
