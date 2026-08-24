# Integration contracts

## Skarbiec

**Outcome:** add and operate a mailbox without copying its credential into Skrzynka.

**Boundary:** the local `skarbiec` CLI and canonical `skarbiec.item.v2` payload. Skrzynka calls `skarbiec list` for metadata, decrypts exact selected items only inside the core process, and writes Gmail authorizations with `skarbiec set-json`. Supported source kinds are `login` and `bundle`.

**Credential scope:** password mailboxes require `username` and `password`. Gmail connection reads only the selected login identity semantically, obtains Google authorization, and writes a dedicated bundle containing `auth_method=oauth2`, `refresh_token`, the OAuth-client item reference, and the automatic Gmail server profile. Secret values remain inside Skarbiec child-process responses and bounded connection tasks; they never enter API responses, SQLite, or logs.

**Lifecycle:** password rotation requires no Skrzynka change because the exact item is resolved for each connection. Gmail reconnect rotates the dedicated authorization bundle while preserving the mailbox reference. Revocation makes later sync/reply attempts fail closed while retained local mail remains readable. Removing a mailbox from Skrzynka does not mutate Skarbiec or provider state.

**Failure isolation:** unavailable or invalid items mark only their mailbox unhealthy. Metadata listing may fail without preventing existing local messages from being read.

**Compatibility:** Skarbiec must emit the documented JSON-first `list` and `get` responses and `skarbiec.item.v2` payloads. Unknown item kinds or non-text credential fields are rejected.

## IMAP

**Outcome:** import new messages from every enabled mailbox.

**Capabilities:** IMAP4rev1 direct TLS, `INBOX`, UID search/fetch, RFC 5322 header parsing, plain-text body extraction, password authentication, and Gmail XOAUTH2. Folder mutation, flags, deletion, push/IDLE, attachments, and remote HTML are unavailable.

**Data:** Skrzynka sends hostname and authentication identity over TLS plus either a password or a short-lived Google access token, then receives bounded MIME messages. It stores selected headers and normalized text. IMAP UID is meaningful only within its mailbox and is namespaced accordingly.

**Reliability:** the current IMAP client path does not configure a network timeout. Authentication, TLS, protocol, malformed-message, and provider-unavailable errors are normalized. Missing, oversized, or unparseable messages are counted as skipped without including provider content in an error. The cursor advances only after the selected batch is committed.

## SMTP

**Outcome:** reply as the mailbox that received an inbound message.

**Capabilities:** authenticated SMTP using required STARTTLS or implicit TLS, password authentication or Gmail XOAUTH2, plain-text bodies, `From`, `To`, `Subject`, `Message-ID`, `In-Reply-To`, and `References`. CC/BCC authoring, attachments, HTML, send scheduling, and provider-specific message APIs are unavailable.

**Data and side effects:** SMTP receives the recipient, subject, thread headers, reply body, authentication identity, and a password or short-lived Google access token. A send is a provider-facing mutation. Skrzynka records intent before the network call and acceptance afterward. A lost terminal response becomes `uncertain` and is never retried automatically.

**Reliability:** connection, authentication, TLS, rate-limit, and rejection errors become a failed attempt. Caller idempotency prevents duplicate calls within Skrzynka, but SMTP has no cross-provider idempotency contract; ambiguous acceptance therefore requires human inspection.

## Google authorization

**Outcome:** selecting a Google identity already present in Skarbiec creates a working Gmail mailbox without copying or reusing its website password.

**Boundary:** the loopback core reads the installed-app OAuth client from `skrzynka-google-oauth-desktop`, creates a ten-minute PKCE flow, and returns Google's canonical authorization URL with `access_type=offline`, `prompt=consent`, and the selected Skarbiec account hint. The desktop opens that URL. Google sends the one-use code directly to the core's loopback callback; the desktop polls only the opaque flow identifier. The core verifies the returned account, exchanges the code, and stores the refresh token in Skarbiec.

**Runtime:** the core refreshes short-lived access tokens directly with Google, caches them in memory until shortly before expiry, and uses XOAUTH2 for both IMAP and SMTP. The OAuth client must be a Google **Desktop app** JSON with an `installed` object and Google's canonical endpoints. A missing or revoked client or refresh token affects only Gmail connection or the corresponding mailbox and requires reconnecting the same profile.

## Desktop client, identity, and onboarding

`skrzynka-desktop` restores user identity and organization selection through `wisent-desktop-auth`, then sends that short-lived bearer and organization ID only to the loopback `/v1` JSON API. The core independently verifies the session and membership before organization-scoped database access. The desktop never reads the SQLite database, invokes Skarbiec, or opens IMAP/SMTP directly. API or identity unavailability fails closed without altering mailbox state.

The desktop runs its first-use journey through Echo's `WisentOnboarding` client. A configured Stado integration transport may read the exact published journey and collect bounded, idempotent onboarding events; a source-identical bundled definition keeps product activation local when that optional transport is unavailable. Neither path carries mailbox credentials or message content.

## Ownership

The Skrzynka maintainers own all adapters in this repository. Skarbiec owns its credential contract; each email provider owns its protocol service and account policy; the desktop repository owns only its API client and presentation.
