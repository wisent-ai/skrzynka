# Message

A message is an immutable, locally normalized inbound email. It is evidence imported from one mailbox, not a live provider object.

## Shape

| Field | Meaning |
|---|---|
| `id` | local UUID |
| `mailbox_id` | receiving mailbox |
| `external_uid` | provider IMAP UID within that mailbox |
| `message_id` | optional RFC Message-ID header |
| `in_reply_to`, `references` | optional thread headers |
| `sender`, `reply_to`, `recipients` | normalized header text |
| `subject` | Subject, or `(no subject)` |
| `sent_at` | parsed provider Date, or `null` |
| `received_at` | local import timestamp |
| `body_text` | first available `text/plain` body, trimmed |
| `snippet` | collapsed whitespace, first 240 characters |

## Creation

A sync searches UIDs after the mailbox cursor and requests each with `BODY.PEEK[]`; it does not set a Seen flag. Raw messages over 2 MiB, missing bodies, or messages that cannot normalize are counted as `skipped`. Skrzynka stores no raw MIME, attachment, or remote HTML.

The database uniqueness key is `(mailbox_id, external_uid)`. Importing the same UID again uses `INSERT OR IGNORE`, so the existing row and UUID remain unchanged.

## Lifecycle

There is no message update or single-message delete interface. A message remains until its mailbox is removed or the database is removed/restored. Message lists are newest local receipt first; `mailbox_id` can filter them. Requested list limits are clamped to 1–500 and default to 100.

A reply targets `reply_to` when present, otherwise `sender`, and carries the inbound Message-ID into `In-Reply-To` and `References`.

## Invariants and refusals

- IMAP UID is meaningful only inside its mailbox.
- Reads are constrained through the mailbox's organization.
- Unknown UUID: `NOT_FOUND` — `message was not found`.
- Invalid API UUID text: `INVALID_IDENTIFIER` — `identifier is not a valid UUID`.
- A malformed provider message can produce `MESSAGE_MALFORMED` with `provider message could not be parsed` or `provider message has no From header`; synchronization skips it rather than exposing provider content.
- A message without a valid reply target is refused as `MESSAGE_REPLY_TARGET_INVALID` — `message has no valid Reply-To or From address`.

See [synchronization](synchronization.md) and [reply attempt](reply-attempt.md).