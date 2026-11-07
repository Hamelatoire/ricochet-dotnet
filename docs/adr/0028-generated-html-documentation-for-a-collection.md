---
status: accepted (built in Core and the CLI; no app entry point yet)
---

# Generated HTML documentation for a collection

Teams want to share what an API does with people who do not use Ricochet, such as product owners or partners. Today a collection can only be shared as files or as an export.

Ricochet may export a collection as a single HTML page, for people who don't use the app. The page shows requests, folders and examples, with no secrets and no live playground.

## Considered Options

- Live playground in the page: rejected at first, because it would send requests from a browser.

## Consequences

- Hosting the page is the user's choice, under ADR 0005, since Ricochet sends nothing.
- Descriptions and examples must be escaped as text.

## Built

`CollectionDocumentation.Render` in `src/Ricochet.Core/CollectionDocumentation.cs` turns one Collection into one HTML page. Folders become nested sections, and each Request shows its method, URL, enabled headers, auth and body. Secret Variables, secret headers and literal credentials (auth tokens, passwords, API key values) are written as `***`, and sensitive URL query values too. Placeholders such as `{{token}}` are shown, since they name a variable and hold no value. All text is HTML-escaped. The page has inline CSS only, with no scripts, external resources or network calls. Hidden folders and Requests (ADR 0032) are left out. `ricochet docs <collection> <file.html>` writes the page from the CLI. It refuses a file inside the workspace folder, and refuses an existing file unless `--force` is given. The app does not call it yet, so there is no UI entry point.
