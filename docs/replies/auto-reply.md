# Automatic replies

`skrzynka mailbox responder <ID> --file <PATH> --every <SPAN> [--from <WHEN>]
[--until <WHEN>]` (or `PUT /v1/mailboxes/:id/auto-reply` with `{body, every,
from, until}`, or **Automatic reply** in Skrzynka Desktop's mailbox
inspector) makes the mailbox answer the mail it receives with the file's
text:

- only mail received from `--from` (an RFC 3339 instant or a span from now
  such as `+2h`; now when omitted) until `--until` (open when omitted) is
  answered;
- one sender is answered at most once within `--every` (a whole number and
  `m`, `h`, `d` or `w`, such as `7d`), counted from the last automatic reply
  that went to that address;
- the reply goes where a reply would (the message's Reply-To, else its From),
  threaded to the message, through the mailbox's SMTP, signed with the
  mailbox's signature and counted against its send limit like any reply; it
  states `Auto-Submitted: auto-replied` (RFC 3834), so other responders do not
  answer it;
- mail is never answered when it came from the mailbox's own address, when
  its headers say a machine sent it (an `Auto-Submitted` other than `no`, any
  `Precedence`, a `List-Id`, or a `multipart/report` delivery report), or
  when its sender was answered within the span. The headers are read from
  the provider by UID, as `message source` reads them.

A running `skrzynka serve` answers each message as soon as an import writes
it: every import and every change to an automatic reply wakes it through the
fleet database, whichever process made the change. `mailbox responder <ID>
--answer` (or `POST /v1/auto-replies/answer`) answers every message of the
organization's mailboxes it has not decided on now, without a running
service.

Each message is decided once: the decision is claimed before anything is
sent, so two processes never answer one message twice, and `mailbox
responder <ID> --decisions` (`GET /v1/mailboxes/:id/auto-reply/decisions`,
Desktop's **Decisions**) lists every decision newest first: the message, the
address, the reply attempt sent (whose delivery state `message replies <ID>`
reads), or why none was. `mailbox responder <ID>` reads the reply and
`--clear` (`PUT` with `null`) removes it; decisions already made stay.

## Refusals

| Code | When |
| --- | --- |
| `NOT_FOUND` | The organization holds no mailbox with that ID. |
| `AUTO_REPLY_INVALID` | The text holds nothing visible, `--every` is not a span such as `7d` or is not ahead, `--from` or `--until` is neither an RFC 3339 instant nor a span from now, or `--until` is not after `--from`. |

A reply the responder tried and the core refused (a send limit reached, an
SMTP refusal) is recorded on the decision as `the reply was refused: <CODE>:
<sentence>`; a message whose headers could not be read from the provider is
recorded with the provider's refusal.

## Real test

`tests/replies/auto-reply.mjs` runs the built CLI against a dedicated real
mailbox and a sender address the operator names (one that accepts the
mailbox's mail). It sets an automatic reply, puts four messages of the run
into INBOX with `message restore` (two from the sender, one stating
`Auto-Submitted`, one from a mailing list), syncs and answers, and checks one
of the sender's messages was answered with a reply attempt that ended `sent`,
the other skipped as answered within the span, the machine and list messages
skipped with why, and that a second pass decides nothing again; bad settings
are refused with `AUTO_REPLY_INVALID`. The mailbox's prior automatic reply is
put back.

```sh
node tests/replies/auto-reply.mjs --organization <ORG> --mailbox <ID> \
  --sender <ADDRESS> --binary <PATH> --source-revision <COMMIT>
```

Reports remain under `.build/real-tests/auto-reply/<run>/report.json`.
