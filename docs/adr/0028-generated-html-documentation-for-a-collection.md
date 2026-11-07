---
status: proposed
---

# Generated HTML documentation for a collection

Teams want to share what an API does with people who do not use Ricochet, such as product owners or partners. Today a collection can only be shared as files or as an export.

Ricochet may export a collection as a single HTML page, for people who don't use the app. The page shows requests, folders and examples, with no secrets and no live playground.

## Considered Options

- Live playground in the page: rejected at first, because it would send requests from a browser.

## Consequences

- Hosting the page is the user's choice, under ADR 0005, since Ricochet sends nothing.
- Descriptions and examples must be escaped as text.
