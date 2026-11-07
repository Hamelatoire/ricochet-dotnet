---
status: proposed
---

# Preview the Request exactly as it will be sent

The Request view shows `{{name}}` text, and the values that replace them are only known when the Request is sent. A person can't check that a header, a token or a body has the right value until the response comes back, and a wrong value may already have reached the server. We will add a "Preview" view that shows the Request with every Variable, function and Secret applied, as the sender will build it. Secrets stay masked, as the curl output in [ADR 0022](0022-generate-a-request-as-code.md) does.

## Decisions within this

- **It shows what the sender builds:** the preview uses the same resolver and builder as a send, so it can't drift from what is sent.
- **Secrets are masked:** a Secret's value is shown as `***`, with its name, following the masking rules in [ADR 0016](0016-reveal-and-confirm-in-the-request-view.md).
- **Nothing is sent by previewing:** opening the preview makes no network call and writes nothing to History.
- **Prompted Variables are answered first:** if a Variable is set to prompt, as [ADR 0019](0019-prompt-for-a-variable-when-a-request-is-sent.md) describes, the preview asks for the answer, so it shows a real value.

## Considered Options

- **Show the Request as written only:** what exists, but it leaves the most useful check to the server's response.
- **Send to a test server to see the result:** realistic, but it still sends a real request, and a test server isn't always available.
- **Show the curl command only:** already exists in [ADR 0022](0022-generate-a-request-as-code.md), but a curl command is not the view people read for headers and bodies.

## Consequences

- The preview must rebuild on every change, so it must be fast enough not to lag typing.
- A built-in function such as a generated ID gives a new value per preview, which the view should say, so the preview isn't mistaken for the value sent.
- The preview is a second view of the same Request, so the tabs in [ADR 0041](0041-open-several-requests-as-tabs.md) must keep it per tab.
