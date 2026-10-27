# Blind copies

A blind copy is a recipient the message is delivered to but whose address no
header of the message names, so the other recipients do not learn of it.

| Surface | Form |
| --- | --- |
| CLI | `skrzynka message send … --bcc <ADDRESS>`, `message forward … --bcc <ADDRESS>`, `message draft save\|update … --bcc <ADDRESS>` (each repeatable, or comma-separated) |
| HTTP | `bcc` beside `to` and `cc` on `POST /v1/mailboxes/:id/outbound`, `POST /v1/messages/:id/forward`, `POST /v1/drafts` and `PUT /v1/drafts/:id` |
| Desktop | **Bcc** on Compose, in the Forward section and in the draft editor |

Blind copies are bare addresses, checked and deduplicated as `--to` and
`--cc` are, before the row is claimed. They are SMTP envelope recipients
only: the message Skrzynka builds carries no `Bcc` header, so neither the
To and Cc recipients nor the blind ones see the address in what they
receive.

The outbound row records them in `bcc` (comma-separated, `null` for none),
so `message outbound` shows who was blind-copied; they are part of the
request, so the same idempotency key with other blind copies is
`IDEMPOTENCY_KEY_REUSED`. `message outbound --to <TEXT>` finds a message by a
To, Cc or Bcc address. A draft keeps its blind copies in `bcc` (`""` for
none) and sends them; `--bcc` given at all replaces every one.

Replies carry no blind copies: a reply answers the people who wrote, and
`--all` copies them openly.

## Refusals

| Code | When |
| --- | --- |
| `OUTBOUND_RECIPIENT_INVALID` | A blind copy is not a valid bare address, as for `--to`. |
| `IDEMPOTENCY_KEY_REUSED` | The key was used with other blind copies. |

## Real test

`tests/sending/bcc.mjs` sends from a dedicated mailbox to a second declared
mailbox with a third declared mailbox as the blind copy. It checks the row is
`sent` and records `bcc` and no `cc`, that `outbound --to` finds it by the
blind address, then syncs both receiving mailboxes until the message is
stored in each and checks that neither copy names the blind address in To or
Cc and that neither original, saved with `message source`, carries a `Bcc`
header. It syncs until the message arrives; with no delivery it does not end.

```sh
node tests/sending/bcc.mjs --organization <ORG> --mailbox <ID> \
  --to-mailbox <ID> --bcc-mailbox <ID> --binary <PATH> --source-revision <COMMIT>
```

Reports remain under `.build/real-tests/send-bcc/<run>/report.json`.
