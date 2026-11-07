# Ricochet User Data Policy

*Last updated: 7 November 2026*

Ricochet is a desktop app for building and sending HTTP requests. It is designed so that your data stays on your computer. This policy describes what the app stores, what it sends over the network, and what it doesn't do.

## Summary

- Ricochet has no accounts, no sign-in and no cloud storage.
- Ricochet collects no telemetry, analytics or crash reports.
- Ricochet makes no background network calls. The only network traffic is the requests you send, and the OpenAPI descriptions you fetch by pressing **Fetch**.
- A mock server you start listens only on your own computer, so other computers can't reach it.
- Your data is stored only on your computer.

## What Ricochet stores on your computer

- **Workspace folder:** your collections, folders, requests, environments and globals, saved as JSON files in a folder you choose. You can put this folder in version control, so anyone you share it with receives it. A new workspace gets a `.gitignore` that keeps your personal files, your `.env` file and your secrets out of git.
- **Personal data:** your history of sent requests, the active environment, the theme choice, the folder you last opened, the window size and position, your request settings (such as timeout, proxy address and client certificate path) and the progress of bulk runs. These are kept in your app data folder, or in the folder named by the `RICOCHET_DATA` environment variable if you set it. They are never written to the shared workspace folder.
- **Secrets:** values you mark as secret, the password for a client certificate, and the values you type into auth fields, are stored in your operating system's secure storage: DPAPI on Windows, the Keychain on macOS, or Secret Service on Linux. Secrets are kept out of the workspace files. A client certificate file is never copied; Ricochet keeps only its path.
- **Captured values:** values saved from responses are kept in memory only, and are cleared when you close the app or the collection.

## What leaves your computer

Ricochet sends network traffic only when you ask it to:

- **Requests you send.** Ricochet sends each request to the address you entered, with the headers, body and auth you set. If you have set an HTTP proxy or a client certificate, the request goes through that proxy and presents that certificate.
- **OpenAPI descriptions you fetch.** Ricochet downloads a description only when you press **Fetch** in the URL dialog. It doesn't refresh descriptions on its own.

Nothing else is sent. Ricochet doesn't send your collections, history, settings or secrets to us or anyone else.

## Imports

When you import a Postman export, an Insomnia export or an OpenAPI description, Ricochet reads the file or the response on your computer. It doesn't upload them anywhere. Secrets found in an import are moved into secure storage.

## Exports

When you export a collection, Ricochet writes a Postman or OpenAPI file to a path you choose, and secret values are never written into it. When you export a workspace, Ricochet writes a ZIP file to a path you choose, outside the workspace folder. The ZIP leaves out your personal data, your history, your secrets and your `.env` file. The files are written only on your computer, and only to the paths you name.

## The mock server

When you start a mock server from an OpenAPI description you picked, it listens on `127.0.0.1`, which is only your own computer, and answers requests from that description. Ricochet reads the description from the file you chose and sends nothing. The server runs only while you have started it, and stops when you stop it.

## Git

If your workspace folder is a git repository, Ricochet runs `git` on your computer to show the branch and how far it is ahead of or behind its upstream. It reads only what git already has locally: it never fetches, pushes or commits. The counts show what git last fetched, so they can be out of date.

## The `ricochet` command-line tool

The CLI reads your workspace, reads a `.env` file in the workspace folder, and reads secrets from this machine's secure storage. It sends the requests you run. It masks secrets in its output and reports, and it doesn't send any other data. The only files it writes are the ones you name, such as an xUnit report, an export or a ZIP archive.

## Your choices

You can delete any data by removing the workspace folder, the app data folder and the secure storage entries that Ricochet created. Uninstalling the app doesn't remove these by itself. Files you exported or wrote with the CLI stay where you put them until you delete them.

## Changes to this policy

If the app starts collecting any data, this policy will be updated before the change is released, and the change will be described in the release notes.

## Contact

Questions about this policy can be raised as an issue at https://github.com/Hamelatoire/ricochet-dotnet/issues.
