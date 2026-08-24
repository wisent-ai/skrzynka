# Reply attempt

A reply attempt is the durable record of one request to answer one stored message. It separates local intent from SMTP acceptance and makes caller retries idempotent.

## Shape

The resource contains `id`, `message_id`, `idempotency_key`, the plain-text `body`, `status`, optional `provider_message_id`, optional `error_code`/`error_message`, and `created_at`, `updated_at`, and optional `sent_at` timestamps.

## State machine

```text
pending -> sending -> sent
                   -> failed
                   -> uncertain
```

Skrzynka inserts `pending` before resolving credentials or opening SMTP, immediately records `sending`, and then records the terminal evidence. `sent` means the configured SMTP server accepted the message; it does not prove final recipient delivery. A task that disappears after send begins becomes `uncertain` with `REPLY_UNCERTAIN`. On database startup, any attempt left in `sending` is also recovered as `uncertain`.

## Idempotency

The idempotency key is database-unique. Repeating the same key with the same message and normalized body returns the existing attempt without sending again. Reusing it for another message or body is refused. A retry after `failed` or `uncertain` must use a new key and therefore expresses a new provider mutation.

CLI callers may omit the key; Skrzynka generates a fresh UUID. Retrying automation should supply and retain its own key.

## Message construction

The reply uses the source mailbox's `From`, targets `Reply-To` or `From`, prefixes `Re:` only when absent, emits a new local Message-ID, and preserves thread headers. The body is UTF-8 plain text and is capped at 256 KiB. SMTP requires STARTTLS or implicit TLS and uses password or Gmail XOAUTH2 credentials resolved for the source mailbox.

## Refusals

- `IDEMPOTENCY_KEY_INVALID` — `idempotency_key must contain 1 to 200 non-whitespace characters`.
- `IDEMPOTENCY_KEY_REUSED` — `idempotency key already belongs to a different reply request`.
- `IDEMPOTENCY_CONFLICT` — `reply request already exists`.
- `REPLY_BODY_INVALID` — `reply body must not be empty`.
- `REPLY_BODY_TOO_LARGE` — `reply body exceeds the 256 KiB limit`.
- `MESSAGE_REPLY_TARGET_INVALID` — `message has no valid Reply-To or From address`.
- `REPLY_MESSAGE_INVALID` — `reply could not be encoded as an email message`.
- `SMTP_TLS_FAILED`, `SMTP_REJECTED`, and `SMTP_UNCERTAIN` describe provider-facing failure boundaries; see the [runbook](../runbook.md).

A mailbox deletion cascades to all of its reply evidence. See [message](message.md).