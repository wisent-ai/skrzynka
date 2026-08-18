# Loopback JSON API

The service listens on `http://127.0.0.1:8788` by default and refuses non-loopback bind addresses. Except for `GET /healthz` and the provider-owned Gmail callback, every route requires `Authorization: Bearer <wisent-session>` and `X-Wisent-Organization-ID: <organization-id>`. The core verifies both with centralized Wisent identity and rejects a selected organization without a matching membership. Durable resources are filtered by the verified organization. All request and response bodies are JSON. Timestamps are RFC 3339 UTC strings. Errors use:

```json
{
  "error": {
    "code": "MAILBOX_PROFILE_INVALID",
    "message": "smtp_host is required",
    "retryable": false
  }
}
```

## Health and status

- `GET /healthz` — process readiness, schema version, product version.
- `GET /v1/status` — database path, counts, poll interval, and Skarbiec availability; no credential resolution.

## Skarbiec metadata

- `GET /v1/skarbiec/items` — metadata returned by `skarbiec list`, limited to item ID, kind, tags, revision count, and state. No field values.

## Gmail profiles and authorization

- `GET /v1/gmail/profiles` — deduplicated Google account addresses and their source Skarbiec item IDs; passwords are never returned.
- `POST /v1/gmail/oauth/start` with `{"skarbiec_item_id":"platform-admin-google"}` — create a ten-minute PKCE flow and return `flow_id`, a trusted Google `authorization_url`, and `expires_at`.
- `GET /v1/gmail/oauth/callback?state=UUID&code=CODE` — provider callback on the same loopback listener. Google invokes it after consent; it consumes the flow, verifies the authorized address, stores the durable authorization in Skarbiec, creates or reuses the Gmail mailbox, and returns only a close-window HTML result.
- `GET /v1/gmail/oauth/{flow_id}` — poll `pending`, `processing`, `completed`, or `failed`. A completed response contains the mailbox; a failed response contains the normalized error.

The desktop app receives only the flow identifier and authorization URL, then polls status. OAuth codes, provider tokens, client secrets, and refresh tokens are never returned through JSON or stored in SQLite. Reconnecting the same account rotates its Skarbiec bundle and returns the existing mailbox.

## Workspace domain-wide delegation

- `GET /v1/gmail/delegation` — whether the service-account item `skrzynka-google-service-account` is readable, plus the account's email, its numeric `client_id`, the delegated scope `https://mail.google.com/`, and the admin-console URL where the grant is made. `configured:false` is a state, not an error.
- `POST /v1/gmail/delegate` with `{"email":"user@domain","display_name":null}` — mint a delegated access token for the address through the RFC 7523 JWT-bearer grant, persist the credential bundle in Skarbiec, and create or return the mailbox. No browser, no consent screen, no refresh token; every token is minted from the service-account key at connection time.

Failure codes: `GOOGLE_DELEGATION_NOT_GRANTED` (the Workspace admin has not granted the client ID; the message carries the exact recovery), `GOOGLE_DELEGATION_REJECTED` (the address is not an active user of a granting Workspace domain — consumer `@gmail.com` addresses can never delegate), `GOOGLE_TOKEN_UNAVAILABLE` (retryable transport failure). Delegation reads mail of any user in the granting domain: the service-account key in Skarbiec is a domain-wide credential and shares the vault's protection boundary.

## Mailboxes

- `GET /v1/mailboxes`
- `POST /v1/mailboxes`
- `GET /v1/mailboxes/{mailbox_id}`
- `PATCH /v1/mailboxes/{mailbox_id}` — enable/disable or update non-secret profile fields.
- `DELETE /v1/mailboxes/{mailbox_id}?confirm=true`
- `POST /v1/mailboxes/{mailbox_id}/sync`
- `POST /v1/sync` — bounded synchronization of every enabled mailbox.

Create body:

```json
{
  "skarbiec_item_id": "team-inbox",
  "display_name": "Team",
  "email": "team@example.invalid",
  "imap_host": "imap.example.invalid",
  "imap_port": 993,
  "smtp_host": "smtp.example.invalid",
  "smtp_port": 587,
  "smtp_security": "starttls",
  "poll_interval_seconds": 60
}
```

Every field except `skarbiec_item_id` is optional when the Skarbiec bundle supplies it. Password and authentication identity fields are intentionally absent.

## Messages and replies

- `GET /v1/messages?mailbox_id=UUID&limit=100&offset=0`
- `GET /v1/messages/{message_id}`
- `GET /v1/messages/{message_id}/replies`
- `POST /v1/messages/{message_id}/replies`

Reply body:

```json
{
  "idempotency_key": "d7fb235a-2177-4fa4-a2e0-e6dfa66a78aa",
  "body": "Thank you. We will get back to you today."
}
```

Reply text is limited to 256 KiB. Reusing the same key returns the existing attempt and does not send again. `sent` means the configured SMTP server accepted the message. `uncertain` means acceptance could not be established and automatic resend is prohibited.

## Bounds and compatibility

List limits cap at 500. Raw messages cap at 2 MiB and each sync imports at most 200 per mailbox. Clients must tolerate additive response fields. Breaking resource or semantic changes use a new API path version under the repository's release policy.
