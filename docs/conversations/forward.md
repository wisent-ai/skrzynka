# Forwarding a message

| Surface | Form |
| --- | --- |
| CLI | `skrzynka message forward <ID> --to <ADDRESS> [--cc <ADDRESS>] --body-file <PATH> [--attach <TYPE=PATH>]… --idempotency-key <KEY>`; `message forward --view <NAME> …` for every message of a saved view |
| HTTP | `POST /v1/messages/:id/forward` with `{idempotency_key, to, cc, body, attachments}`; `POST /v1/views/:name/forward` with the same and `template`, `values` in place of `body` if wanted |
| Desktop | **Forward…** in the message inspector, after a confirmation; **Attach file…** adds your own files; **Answer or forward a view** (**Forwarded**) in the inbox inspector while no message is selected |

Forwarding sends a stored message on from the mailbox that received it:

1. The original is read from the provider by UID with `BODY.PEEK[]`, only
   while that UID still holds the message Skrzynka stored. Skrzynka keeps no
   copy of it.
2. A new message is originated from the receiving mailbox exactly as
   `message send` does: subject `Fwd: <original subject>` (`Fwd:` alone when
   the original states none), your text as the body, and the original
   attached whole as `<ID>.eml`, typed `message/rfc822`, then each file of
   your own given with `--attach TYPE=PATH` (`attachments` as
   `[{filename, content_type, content_base64}]` over HTTP). Your files are
   checked as `message send` checks them before the provider is read, so one
   that could never be sent is refused with `OUTBOUND_ATTACHMENT_INVALID`
   first.

Because it is originated mail, a forward is claimed by its idempotency key
before anything reaches SMTP (repeating the key answers the first attempt),
counts against the mailbox's daily send limit, ends `sent`, `failed` or
`uncertain`, appears in `message outbound` and `mailbox delivery`, and a
bounce report about it is matched like any other send. The original's bytes
are not kept; the outbound row records the attachment's name, type, size and
SHA-256 of the original and of each of your files.

## Forwarding every message of a view

`message forward --view <NAME> --to <ADDRESS> --body-file <PATH> |
--template <NAME> [--set KEY=VALUE]… --idempotency-key <KEY>` forwards every
message the saved view lists, such as every invoice to an accountant, each
its own forward exactly as above, claimed by `<KEY>-<message id>`: the same
command again answers the same rows and sends nothing twice, and a message
added to the view later is forwarded by it then. A template is filled for
each message, `{{sender}}`, `{{sender_name}}` and `{{subject}}` from that
message where `--set` gives none. It prints `{view, forwarded, refused}`:
the outbound row of each forward, and each message not forwarded with its
code and sentence (its original gone from the provider, a recipient
suppressed or bounced, the send limit reached), the others still sent.
A key that is blank or holds whitespace is `IDEMPOTENCY_KEY_INVALID`, an
unknown view `VIEW_NOT_FOUND`, and over HTTP a request naming both or
neither of `body` and `template` `FORWARD_BODY_INVALID`, before anything is
sent; `<ID>` with `--view` is refused by Clap. Desktop: **Answer or forward
a view** with **Each message is** **Forwarded** chooses the view, **To**, the
text or a **Template**, and forwards after a confirmation, keeping one key
until every forward succeeds.

## Refusals

| Code | When |
| --- | --- |
| `IDEMPOTENCY_KEY_REQUIRED` | The CLI was given no `--idempotency-key`. |
| `OUTBOUND_FILE_INVALID` | The body file could not be read, is not a regular file, or is not UTF-8. |
| `OUTBOUND_ATTACHMENT_INVALID` | An `--attach` value names no type or no readable file, or a file has a bad name, type or base64. |
| `NOT_FOUND` | The organization holds no message with that ID. |
| `MESSAGE_SOURCE_GONE` | The provider no longer holds the original under its UID. |
| `MESSAGE_SOURCE_CHANGED` | The UID now holds a different message. |
| `OUTBOUND_RECIPIENT_INVALID`, `OUTBOUND_BODY_INVALID`, `IDEMPOTENCY_KEY_REUSED`, `MAILBOX_SEND_LIMIT_REACHED`, SMTP outcomes | As for `message send`. |

## Real test

`tests/conversations/forward.mjs` runs the built CLI against a dedicated real
mailbox and a recipient the operator names. It forwards the newest stored
message with a file of its own, checks the outbound row is `sent` with subject `Fwd: …`, a
`message/rfc822` attachment whose size and SHA-256 equal the original
`message source` saves and then the own file as it was read, checks that
repeating the key answers the same row, that a file without a type is
refused with `OUTBOUND_ATTACHMENT_INVALID`, and that an unknown message is
refused with `NOT_FOUND`. It then tags that message with a tag drawn for the
run, saves a view listing that tag, and checks `forward --view` sends
exactly one forward claimed by `<KEY>-<message id>`, that the same command
again answers the same row, that an unknown view is `VIEW_NOT_FOUND` and a
message ID with `--view` is refused; it deletes the view and removes the
tag before it ends.

```sh
node tests/conversations/forward.mjs --organization <ORG> --mailbox <ID> \
  --recipient <ADDRESS> --binary <PATH> --source-revision <COMMIT>
```

Reports remain under `.build/real-tests/message-forward/<run>/report.json`.
