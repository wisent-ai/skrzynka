# Integration contracts

## Skarbiec

**Outcome:** add and operate a mailbox without copying its credential into Skrzynka.

**Boundary:** the local `skarbiec` CLI and canonical `skarbiec.item.v2` payload. Skrzynka calls `skarbiec list` for metadata and `skarbiec get EXACT_ID` only at the trusted connection boundary. Item IDs are selected explicitly; they are not parsed for discovery. Supported kinds are `login` and `bundle`.

**Credential scope:** the current local-owner mode relies on the OS user and local Skarbiec owner key. Skrzynka requests only the selected item. The item must contain `username` and `password`; a bundle may also contain non-secret connection fields. Values exist only in the child-process response and connection task, never in durable configuration, API responses, or logs.

**Lifecycle:** create, rotate, revoke, recover, and delete credentials in Skarbiec. Rotation requires no Skrzynka change because the exact item is resolved for each connection. Revocation makes later sync/reply attempts fail closed while retained local mail remains readable. Removing a mailbox from Skrzynka does not mutate Skarbiec.

**Failure isolation:** unavailable or invalid items mark only their mailbox unhealthy. Metadata listing may fail without preventing existing local messages from being read.

**Compatibility:** Skarbiec must emit the documented JSON-first `list` and `get` responses and `skarbiec.item.v2` payloads. Unknown item kinds or non-text credential fields are rejected.

## IMAP

**Outcome:** import new messages from every enabled mailbox.

**Capabilities:** IMAP4rev1 direct TLS, `INBOX`, UID search/fetch, RFC 5322 header parsing, and plain-text body extraction. Folder mutation, flags, deletion, push/IDLE, OAuth, attachments, and remote HTML are unavailable.

**Data:** Skrzynka sends hostname, username, and password over TLS and receives bounded MIME messages. It stores selected headers and normalized text. IMAP UID is meaningful only within its mailbox and is namespaced accordingly.

**Reliability:** connections have bounded timeouts. Authentication, TLS, protocol, malformed-message, and provider-unavailable errors are normalized. A message is skipped only when it cannot be parsed within bounds; the mailbox error names the failure without including provider content. The cursor advances only through committed UIDs.

## SMTP

**Outcome:** reply as the mailbox that received an inbound message.

**Capabilities:** authenticated SMTP using required STARTTLS or implicit TLS, plain-text bodies, `From`, `To`, `Subject`, `Message-ID`, `In-Reply-To`, and `References`. CC/BCC authoring, attachments, HTML, OAuth, send scheduling, and provider-specific APIs are unavailable.

**Data and side effects:** SMTP receives the recipient, subject, thread headers, reply body, username, and password. A send is a provider-facing mutation. Skrzynka records intent before the network call and acceptance afterward. A lost terminal response becomes `uncertain` and is never retried automatically.

**Reliability:** connection, authentication, TLS, rate-limit, and rejection errors become a failed attempt. Caller idempotency prevents duplicate calls within Skrzynka, but SMTP has no cross-provider idempotency contract; ambiguous acceptance therefore requires human inspection.

## Desktop client

`skrzynka-desktop` uses only the loopback `/v1` JSON API. It never reads the SQLite database, invokes Skarbiec, or opens IMAP/SMTP directly. API unavailability leaves the core service and CLI usable and does not alter mailbox state.

## Ownership

The Skrzynka maintainers own all adapters in this repository. Skarbiec owns its credential contract; each email provider owns its protocol service and account policy; the desktop repository owns only its API client and presentation.
