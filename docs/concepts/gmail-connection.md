# Gmail connection

A Gmail connection is a Skarbiec-backed authentication adapter for the same IMAP/SMTP mailbox model. Message transport remains Gmail IMAP and SMTP; Skrzynka does not use a provider message API.

## Desktop OAuth lifecycle

1. `GET /v1/gmail/profiles` lists deduplicated Google-looking Skarbiec login identities without secrets.
2. `POST /v1/gmail/oauth/start` resolves the selected identity and the fixed `skrzynka-google-oauth-desktop` item.
3. Skrzynka creates a ten-minute PKCE flow and returns its UUID, expiration, and trusted Google authorization URL.
4. Google redirects the one-use code to the core's loopback callback.
5. The core exchanges the code, looks up the authorized account email, and rejects an account mismatch.
6. It writes a dedicated `skrzynka-gmail-<email-digest>` bundle into Skarbiec and creates or reuses the mailbox.
7. The desktop polls the flow as `pending`, `processing`, `completed`, or `failed`.

The OAuth client must be an `installed` Desktop-app JSON. Authorization and token endpoints must exactly match Google's canonical HTTPS endpoints. Codes, client secrets, access tokens, and refresh tokens never cross the JSON API or enter SQLite.

## Workspace delegation lifecycle

The fixed `skrzynka-google-service-account` item contains a canonical Google service-account JSON. `gmail delegation` reports whether it is readable, plus its numeric client ID, scope `https://mail.google.com/`, and the admin-console grant URL. Missing configuration is returned as `configured:false`.

`gmail delegate --email <user>` signs a one-hour RFC 7523 JWT assertion with that key, asks Google for a delegated token, saves a non-secret-reference Gmail bundle, and creates or reuses the mailbox. There is no consent screen or refresh token. A token is minted again from the service-account key when needed.

## Invariants

- Callback base URL is credential-free loopback HTTP.
- Authorization host is `accounts.google.com`; token host is Google's canonical OAuth endpoint.
- OAuth flow state belongs to one organization and is single-consumption.
- Access tokens are memory-only and reused only until 60 seconds before expiry.
- Consumer `@gmail.com` identities cannot use Workspace domain-wide delegation.

## Refusal families

OAuth uses `GMAIL_OAUTH_CLIENT_INVALID`, `GMAIL_OAUTH_STATE_MISSING`, `GMAIL_OAUTH_STATE_INVALID`, `GMAIL_OAUTH_CODE_INVALID`, `GMAIL_OAUTH_FLOW_EXPIRED`, `GMAIL_OAUTH_FLOW_CONSUMED`, `GMAIL_OAUTH_REJECTED`, `GMAIL_OAUTH_RESPONSE_INVALID`, `GMAIL_REFRESH_TOKEN_MISSING`, `GMAIL_IDENTITY_UNAVAILABLE`, and `GMAIL_OAUTH_ACCOUNT_MISMATCH`.

Runtime refresh uses `GMAIL_TOKEN_REFRESH_UNAVAILABLE`, `GMAIL_TOKEN_RESPONSE_INVALID`, and `GMAIL_AUTHORIZATION_EXPIRED`. Delegation uses `GOOGLE_TOKEN_UNAVAILABLE`, `GOOGLE_DELEGATION_NOT_GRANTED`, and `GOOGLE_DELEGATION_REJECTED`. Exact operator actions are in the [runbook](../runbook.md).