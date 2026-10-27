# Replying to everyone

`skrzynka message reply <ID> --body-file <PATH> --all --idempotency-key <KEY>`
(HTTP: `reply_all: true` in the body of `POST /v1/messages/:id/replies`;
Skrzynka Desktop: **Reply to all** in the reply composer) answers the message
as a plain reply does, to its Reply-To or else its From address, and also
copies (Cc) every address of the message's To and Cc headers:

- each address once, compared without case;
- never the mailbox's own address, and never the reply's target again;
- display names as the message wrote them.

Each import records the message's Cc header in its `cc` field. A message
imported before Cc was recorded has `cc: null` and is answered on its To
addresses; reading its attachments (`message attachment list`) records its
details, Cc included.

`reply_all` is part of the request: the attempt records it (`reply_all` on
the row `message replies` lists), and the same idempotency key with the
other choice is refused with `IDEMPOTENCY_KEY_REUSED`. The reply is held to
the mailbox's send limit and carries its signature like any reply.

## Copying more addresses

`message reply … --cc <ADDRESS> --bcc <ADDRESS>` (repeatable or
comma-separated; HTTP: `cc` and `bcc` arrays; Desktop: **Also copy (Cc)** and
**Blind copy (Bcc)** in the composer) copies more addresses on the reply,
with or without `--all`. Each is a bare address, checked before anything is
claimed (`OUTBOUND_RECIPIENT_INVALID` names one that is not) and kept once.
Cc addresses are named in the reply's Cc header; blind copies are SMTP
envelope recipients only and named in no header, as for
[originated mail](bcc.md). The attempt records them comma-separated as `cc`
and `bcc` (`null` for none), and the same key with other copies is
`IDEMPOTENCY_KEY_REUSED`.

## Quoting the message

`message reply … --quote` (HTTP: `quote: true`; Desktop: **Quote the message
below the reply** in the composer) puts the message's stored text below the
reply, after a line `On <WHEN>, <SENDER> wrote:` (its Date header's instant,
or when Skrzynka received it when it states none), each of its lines after
`> ` and an empty one as `>`. The signature follows the quote. The attempt's
`body` is the text as sent, quote included, so the same key with or without
`--quote` is `IDEMPOTENCY_KEY_REUSED`. `tests/writing/follow-ups.mjs` sends
a quoted reply and checks it.

## Answering every message of a view

`message reply --view <NAME> --body-file <PATH> | --template <NAME>
[--set KEY=VALUE]… [--quote] [--all] [--cc] [--bcc] --idempotency-key <KEY>`
(HTTP: `POST /v1/views/:name/replies` with `{idempotency_key, body |
template, values, attachments, reply_all, quote, cc, bcc}`, role `member`;
Desktop: **Answer or forward a view** in the inbox inspector, **Each message
is** **Answered**) answers every message the saved view lists, such as
acknowledging every application at once, each with its own reply exactly
as `message reply <ID>` sends one, claimed by `<KEY>-<message id>`: the same
command again answers the same attempts and sends nothing twice. A template
is filled for each message, `{{sender}}`, `{{sender_name}}` and
`{{subject}}` from that message where `--set` gives none. It prints `{view,
replied, refused}`: each reply's attempt, and each message not answered with
its code and sentence (a placeholder nothing fills for it, a recipient
suppressed, the send limit reached), the others still answered. A key that
is blank or holds whitespace is `IDEMPOTENCY_KEY_INVALID`, an unknown view
`VIEW_NOT_FOUND`, and over HTTP a request naming both or neither of `body`
and `template` `REPLY_BODY_INVALID`, before anything is sent; `<ID>` with
`--view` is refused by Clap. `tests/writing/follow-ups.mjs` answers a view
of one message the mailbox sent itself and checks it.

## Refusals

| Code | When |
| --- | --- |
| `MESSAGE_REPLY_ALL_INVALID` | The message's To or Cc header does not read as a list of addresses; the header is named. Nobody is left out unsaid. |
| `OUTBOUND_RECIPIENT_INVALID` | A `--cc` or `--bcc` value is not a bare address. |
| `MESSAGE_REPLY_TARGET_INVALID` | The Reply-To or From address is not valid, as for every reply. |

Every other refusal is that of `message reply`.

## Real test

`tests/writing/reply-all.mjs` runs the built CLI against a dedicated real
mailbox. It needs a stored message whose To or Cc names at least one address
besides the mailbox's own, and the operator's agreement that replying to all
of them is acceptable (`--confirm-recipients`), and two addresses that accept
a copy (`--copy`, `--blind-copy`). It replies with `--all`,
checks the attempt is `sent` with `reply_all: true`, checks repeating the key
without `--all` is refused with `IDEMPOTENCY_KEY_REUSED`, and checks the
repeated key with `--all` answers the same attempt. It then replies with
`--cc` and `--bcc`, checks the attempt is `sent` and records both, that the
same key with fewer copies is `IDEMPOTENCY_KEY_REUSED`, and that a copy that
is no address is refused with `OUTBOUND_RECIPIENT_INVALID`.

```sh
node tests/writing/reply-all.mjs --organization <ORG> --mailbox <ID> \
  --message <MESSAGE_ID> --confirm-recipients --copy <ADDRESS> --blind-copy <ADDRESS> \
  --binary <PATH> --source-revision <COMMIT>
```

Reports remain under `.build/real-tests/reply-all/<run>/report.json`.
