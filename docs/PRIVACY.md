# Ricochet User Data Policy

*Last updated: 6 November 2026*

Ricochet is a desktop app for building and sending HTTP requests. It is designed so that your data stays on your computer. This policy describes what the app stores, what it sends over the network, and what it doesn't do.

## Summary

- Ricochet has no accounts, no sign-in and no cloud storage.
- Ricochet collects no telemetry, analytics or crash reports.
- Ricochet makes no background network calls. The only network traffic is the requests you send, and the OpenAPI descriptions you fetch by pressing **Fetch**.
- Your data is stored only on your computer.

## What Ricochet stores on your computer

- **Workspace folder:** your collections, folders, requests, environments and globals, saved as JSON files in a folder you choose. You can put this folder in version control, so anyone you share it with receives it.
- **Personal data:** your history of sent requests, the active environment, the theme choice and the folder you last opened. These are kept in your app data folder, or in the folder named by the `RICOCHET_DATA` environment variable if you set it. They are never written to the shared workspace folder.
- **Secrets:** values you mark as secret, and the values you type into auth fields, are stored in your operating system's secure storage: DPAPI on Windows, the Keychain on macOS, or Secret Service on Linux. Secrets are kept out of the workspace files.
- **Captured values:** values saved from responses are kept in memory only, and are cleared when you close the app or the collection.

## What leaves your computer

Ricochet sends network traffic only when you ask it to:

- **Requests you send.** Ricochet sends each request to the address you entered, with the headers, body and auth you set.
- **OpenAPI descriptions you fetch.** Ricochet downloads a description only when you press **Fetch** in the URL dialog. It doesn't refresh descriptions on its own.

Nothing else is sent. Ricochet doesn't send your collections, history, settings or secrets to us or anyone else.

## Imports

When you import a Postman export or an OpenAPI description, Ricochet reads the file or the response on your computer. It doesn't upload them anywhere. Secrets found in an import are moved into secure storage.

## The `ricochet` command-line tool

The CLI reads your workspace and sends the requests you run. It masks secrets in its output and reports, and it doesn't send any other data.

## Your choices

You can delete any data by removing the workspace folder, the app data folder and the secure storage entries that Ricochet created. Uninstalling the app doesn't remove these by itself.

## Changes to this policy

If the app starts collecting any data, this policy will be updated before the change is released, and the change will be described in the release notes.

## Contact

Questions about this policy can be raised as an issue at https://github.com/Hamelatoire/ricochet-dotnet/issues.
