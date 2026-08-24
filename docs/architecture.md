# Architecture

Skrzynka is one local Rust process around four boundaries: SQLite for durable local mail state, Skarbiec for secrets, IMAP/SMTP for provider transport, and central Wisent identity for organization-scoped HTTP access.

```text
CLI (legacy-local) ───────┐
                         v
Desktop -> loopback HTTP -> AppState -> SQLite
                |            |  |
                |            |  +-> Skarbiec child -> credential JSON
                |            |                       (connection lifetime)
                |            +----> IMAP read / SMTP reply
                +-> central identity verification
Google browser -> loopback OAuth callback -> Google token/identity endpoints
```

## What Skrzynka owns

- mailbox references and non-secret connection profiles;
- organization-scoped SQLite rows;
- synchronization cursors and normalized message copies;
- reply intent, idempotency, acceptance evidence, and uncertain recovery;
- loopback `/v1` enforcement and provider adapters in this repository.

## What it does not own

- **Skarbiec:** encryption, vault availability, credential recovery, rotation, and revocation;
- **mail provider:** mailbox existence, IMAP UIDs, folders and flags, SMTP acceptance policy, delivery, and provider retention;
- **shared identity:** users, bearer sessions, and organization membership;
- **skrzynka-desktop:** presentation and first-use screens;
- **Echo/Stado:** optional onboarding definition/event transport.

Skrzynka does not generate replies, manage folders, mark or delete provider messages, retain attachments or remote HTML, or expose a remote shared service.

## Durable state

SQLite schema 2 uses WAL and foreign keys. `mailboxes` holds an organization and Skarbiec reference; `messages` cascade from mailboxes and are unique by mailbox/UID; `reply_attempts` cascade from messages and have a globally unique idempotency key. Startup migrates schema 1 rows to `legacy-local` and turns interrupted `sending` replies into `uncertain`.

The database contains message and reply bodies and must be treated as sensitive. It does not contain mailbox passwords, OAuth tokens, Wisent sessions, or membership records. Backups need the same protection as the inbox.

## Concurrency and scheduling

One `AppState` owns an async operation mutex. Synchronizations and SMTP work are serialized in-process; network calls do not hold a SQLite transaction. The background task wakes every 15 seconds, selects due enabled mailboxes by their per-mailbox interval, and processes them in sequence. Separate processes sharing one database are unsupported.

SQLite serializes its connection behind a mutex and uses a five-second busy timeout. WAL enables durable companion files, which must be included in a stopped-service backup when present.

## Trust boundaries

### Loopback HTTP

`serve` rejects non-loopback IPs before binding. Loopback is an exposure boundary, not authentication: all `/v1` resources except Google's callback still require a verified bearer and organization header. Health exposes only versions/readiness.

### Credentials

The exact Skarbiec item is fetched only when profile validation or a connection needs it. Child execution is bounded to 15 seconds and 2 MiB. Secrets reside transiently in core memory and the connection task. The API models and schema contain no password field.

### Provider network

IMAP uses direct TLS and `BODY.PEEK[]`; synchronization is read-only but the current IMAP library call has no configured network timeout. SMTP uses required STARTTLS or implicit TLS and a 30-second transport timeout. SMTP submission is a real external mutation; an ambiguous terminal response is never retried automatically.

### Gmail

OAuth callback state binds the external browser redirect to one in-memory organization flow. Endpoint allowlists require canonical Google hosts. Workspace delegation grants the service-account client ID access to every authorized user in the domain, so its Skarbiec item is a domain-wide credential.

## Failure isolation

All-mailbox sync embeds each mailbox failure and continues. Local messages remain readable when Skarbiec, identity, or a provider is unavailable. HTTP identity verification fails closed. A failed SMTP attempt is durable; an ambiguous attempt becomes `uncertain` so recovery cannot silently duplicate mail.