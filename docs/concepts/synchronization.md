# Synchronization

Synchronization is one bounded, read-only import from an enabled mailbox's IMAP `INBOX` into SQLite.

## One pass

1. Load the mailbox and resolve its selected Skarbiec credential.
2. Open a direct-TLS IMAP connection.
3. Authenticate with username/password or Gmail XOAUTH2.
4. Select `INBOX` and search `UID max(last_uid + 1, 1):*`.
5. Sort UIDs, keep at most 200, and fetch each with `BODY.PEEK[]`.
6. Normalize messages of at most 2 MiB and insert them by `(mailbox_id, UID)`.
7. Record the highest processed UID, successful timestamp, and cleared error.

There is no IMAP IDLE, folder selection, flag mutation, deletion, or attachment download. The current IMAP client path does **not** configure a network timeout; operators must not rely on the stale 30-second claim. SMTP separately has a 30-second transport timeout.

## Output

A single-mailbox sync returns `mailbox_id`, `received`, `skipped`, `last_uid`, and `completed_at`, or fails with one error envelope. An all-mailbox sync returns a top-level completion time and one result per enabled mailbox; a mailbox failure is embedded with `ok:false` and does not make the command fail or stop later mailboxes.

## Scheduling

`serve` wakes its scheduler every 15 seconds. A mailbox is due when enabled and either never synchronized or at least its own `poll_interval_seconds` past `last_sync_at`. The default interval is 60 seconds and valid range is 15–86400. One in-process operation lock serializes sync and reply work; concurrent processes on one database are unsupported.

## Cursor semantics

The cursor advances to the highest UID considered in the batch, including bounded messages that were skipped because the provider omitted a body, exceeded 2 MiB, or failed normalization. It advances only after inserts complete and success is recorded. A connection, authentication, search, or fetch failure records the normalized mailbox error and leaves `last_sync_at` unchanged.

## Refusals

- `IMAP_UNAVAILABLE` — `IMAP server could not be reached over TLS`.
- `IMAP_AUTHENTICATION_FAILED` — password or Gmail authorization was refused.
- `IMAP_INBOX_UNAVAILABLE` — `the provider did not make INBOX available`.
- `IMAP_SEARCH_FAILED` — `IMAP UID search failed`.
- `IMAP_FETCH_FAILED` — `IMAP fetch failed at UID {uid}`.
- Credential-resolution failures occur before the IMAP connection and are recorded on the mailbox.

See [mailbox](mailbox.md) and the [runbook](../runbook.md).