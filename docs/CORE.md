# Core behavior contract

## Public resources

### Mailbox

A mailbox is identified by a generated UUID and contains an exact `skarbiec_item_id`, display name, sending address, IMAP/SMTP host and port, SMTP security mode, enabled state, last persisted IMAP UID, synchronization timestamps, and the latest normalized error. Authentication fields are never part of this resource.

Valid state changes are `enabled → disabled → enabled` and `present → removed`. Synchronization updates its cursor only after every selected message has been committed. Removing a mailbox cascades only through Skrzynka's local messages and reply attempts.

### Message

An inbound message is immutable normalized content keyed by `(mailbox_id, IMAP UID)`. Headers include provider Message-ID, threading headers, sender, recipients, subject, provider date, receive time, text body, and a bounded snippet. Repeated synchronization of the same UID is a no-op.

### Reply attempt

A reply attempt belongs to one inbound message and one caller-supplied idempotency key. States are:

```text
pending -> sending -> sent
                   -> failed
                   -> uncertain (recovered after interrupted sending)
```

The same idempotency key returns its existing attempt and never sends twice. A retry after `failed` or `uncertain` requires a new key and is therefore an explicit new provider mutation. SMTP acceptance is the `sent` boundary; delivery to the recipient's final inbox is outside Skrzynka's authority.

## Invariants

- One mailbox always replies through its own Skarbiec item and SMTP profile.
- No API or database field accepts a mailbox password.
- Secret resolution occurs immediately before IMAP/SMTP authentication and the value is dropped with the connection task.
- API startup is loopback-only.
- Message import is bounded to 200 messages per mailbox per synchronization and 2 MiB per raw message.
- One mailbox failure does not stop synchronization of other mailboxes.
- Provider-side messages, flags, folders, and deletion are never mutated by synchronization.
- Unknown configuration and unsupported SMTP security values are rejected before persistence.

## Concurrency and durability

SQLite runs in WAL mode with foreign keys enabled. Mailbox and reply idempotency constraints are database-enforced. Synchronizations for one mailbox are serialized in-process; concurrent service processes are unsupported. A transaction owns each local resource change. Network calls never hold a database transaction open.

## Error classes

Public errors contain a stable `code`, a safe `message`, whether retry may help, and an HTTP status. Credential values, provider payloads, message bodies, and stack traces are excluded. Invalid input is 400, missing resources 404, stale/idempotency conflicts 409, dependency failures 502 or 503, and unexpected local failures 500.

## Retention and privacy

Skrzynka retains normalized messages and reply bodies until their mailbox is removed or the database is removed. There is no automatic retention deletion in schema 1. Raw MIME, attachments, and remote HTML are not stored. Operators must treat the SQLite file and its backups as sensitive message data even though it contains no mailbox passwords.

## Resource behavior

Each mailbox sync uses one blocking IMAP session with a 30-second network timeout and imports at most 200 messages. Polling defaults to 60 seconds and never overlaps a previous poll loop. API message lists default to 100 and cap at 500. Body and reply sizes are bounded at 2 MiB and 256 KiB respectively. Over-limit data is rejected or truncated at the documented boundary, never loaded without a limit.
