# Reading what was sent

Every reply Skrzynka makes is stored as a reply attempt of the message it
answers, and every message it originates as an outbound message. Both keep
their delivery state (`pending`, `sending`, `sent`, `failed` or `uncertain`),
the provider message ID once sent, and the error the provider or transport
gave. The same reads exist on every surface:

| Read | CLI | API | Desktop |
|---|---|---|---|
| A message's reply attempts, newest first | `skrzynka message replies ID` | `GET /v1/messages/:id/replies` | Replies |
| Every reply attempt, newest first, or oldest first with `--oldest-first` | `skrzynka message replies [--mailbox M] [--status STATUS] [--since WHEN] [--until WHEN] [--contains TEXT] [--attachment TEXT] [--oldest-first]` | `GET /v1/replies?mailbox_id=&status=&since=&until=&contains=&attachment=&oldest_first=true` | **Replies sent** in the Outbound inspector while no message is selected, with **Only since**, **Only before**, **Oldest first**, **Text contains** and **File name contains** |
| Reply attempts as a CSV table (RFC 4180) without their text, to a new file: `id, message_id, cc, bcc, status, created_at, sent_at, provider_message_id, error_code, error_message, attachments` | `skrzynka message replies [ID or the listing's flags] --csv PATH` (prints `{path, replies, size_bytes}`; `REPLY_OUTPUT_EXISTS` for an existing path, `REPLY_OUTPUT_INVALID` for one that cannot be checked or written) | `GET /v1/replies/csv` (the listing's parameters), `text/csv` offered as `replies.csv` | **Export as a table (.csv)…** in **Replies sent** |
| Outbound messages, newest first, or oldest first with `--oldest-first` (pages cut in that order) | `skrzynka message outbound [--mailbox M] [--to TEXT] [--contains TEXT] [--attachment TEXT] [--status STATUS] [--since WHEN] [--until WHEN] [--bounced] [--answered \| --unanswered] [--oldest-first] [--limit N] [--offset N]` | `GET /v1/outbound?to=&contains=&attachment=&status=&since=&until=&bounced=true&answered=true\|false&oldest_first=true`, `GET /v1/mailboxes/:id/outbound` (same parameters) | Outbound, and **Find sent mail** while no message is selected, with **Oldest first** |
| One outbound message | `skrzynka message outbound --id ID` | `GET /v1/outbound/:id` | its row in Outbound |
| Outbound messages as a CSV table (RFC 4180) without their bodies, to a new file: `id, mailbox_id, recipients, cc, bcc, subject, status, created_at, sent_at, provider_message_id, error_code, error_message, attachments` (the file names, `; ` between them) | `skrzynka message outbound [the listing's flags] --csv PATH` (prints `{path, messages, size_bytes}`; `OUTBOUND_OUTPUT_EXISTS` for an existing path, `OUTBOUND_OUTPUT_INVALID` for one that cannot be checked or written) | `GET /v1/outbound/csv` (the listing's parameters), `text/csv` offered as `outbound.csv` | **Export as a table (.csv)…** in **Find sent mail**, for the filter applied |

None of them resolves a credential or contacts a provider. A message nobody
replied to has `[]` as its replies. Without an ID, `message replies` lists
every reply sent from the organization's mailboxes, or from one with
`--mailbox`, whichever message each answered; `--status` keeps those in one
delivery state, and `--since` and `--until` those claimed at or after and
before an RFC 3339 instant or a `YYYY-MM-DD` day read as its UTC start (a
window whose start is not before its end is `MESSAGE_FILTER_INVALID`), and
`--contains` those whose text contains the given text, case aside (a blank
one is `MESSAGE_FILTER_INVALID`), and `--attachment` those that carried a
file whose recorded name contains the text, case aside (a blank one is
`MESSAGE_FILTER_INVALID`). An ID cannot be given with any of them.

## Finding sent mail

`--to` keeps the outbound messages whose To, Cc or Bcc contains the text, without
case; `--contains` those whose subject or body contains it; `--attachment`
those that carried a file whose recorded name contains it, without case;
`--status` those whose delivery ended, or stands, in that state; `--since`
and `--until` those claimed (`created_at`) at or after and before an RFC 3339
instant or a `YYYY-MM-DD` day read as its UTC start; `--bounced` (Desktop's
**Bounced only**) those a stored bounce report, in any mailbox of the
organization, returned: the report's returned Message-ID is the one the
message went out with, as `mailbox delivery` matches them; `--answered`
those a stored message of the sending mailbox answers, its In-Reply-To or
References naming the Message-ID the message went out with (as a
[follow-up](../writing/follow-ups.md) reads an answer; a bounce report is no
answer), and `--unanswered` those nothing answers yet; the two exclude each
other. They
combine, and `--limit` and `--offset` page what they keep. Nothing matching
is `[]`. The API takes the same filters as the query parameters `to`,
`contains`, `attachment`, `status`, `since`, `until`, `bounced=true` and
`answered=true|false`; Desktop's Outbound
inspector shows them as **Find sent mail** (with **File name contains**,
**Sent since**, **Sent before**, **Bounced only** and **Answer**) while no message is
selected, with **Apply** and **Clear**.

## Sending failures again

`skrzynka message outbound <the listing's flags> --resend-failed
--idempotency-key <KEY>` (HTTP `POST /v1/outbound/resend` with the listing's
parameters and `{idempotency_key}`, role `member`; Desktop: **Send the failed
ones again…** in **Find sent mail**, after a confirmation) originates again,
from the same mailbox, every message the listing keeps whose send ended
`failed`, such as those refused while the mailbox's server was down: its To,
Cc, Bcc, subject and text as written, with the mailbox's signature now. Each
is claimed by `<KEY>-<outbound id>`, so the same request again sends nothing
twice, and prints `{sent, refused}`: the new outbound row of every message
sent, and `{outbound_id, code, message}` for each failed message not sent.
A message that carried files is refused for itself with
`OUTBOUND_RESEND_INVALID`, because Skrzynka keeps only the files' names and
digests ([`draft again`](../writing/drafts.md) starts a draft from it to
attach them); one a send refuses now, such as a
[suppressed](../outreach/suppression.md) or bounced recipient or the
[send limit](../delivery/limit.md) reached, is refused with that send's code.
The failed rows stay as they were, so the listing keeps both. A listing
narrowed by no mailbox, addressee, text, file name or window is
`OUTBOUND_RESEND_INVALID` before anything is read, so nothing sends every
failure ever again at once; one keeping no failed message is `NOT_FOUND`; a
blank key is `IDEMPOTENCY_KEY_INVALID`. Desktop keeps one key until every
listed failure was sent, so a second press repeats the same sends.

`skrzynka message replies <the listing's flags> --resend-failed
--idempotency-key <KEY>` (HTTP `POST /v1/replies/resend` with the
listing's parameters and `{idempotency_key}`, role `member`; Desktop: **Send
the failed ones again…** in **Replies sent**, after a confirmation) does the
same for replies: every reply the listing keeps that ended `failed` is sent
again to the message it answered, with its text as sent (a quote included),
its reply-to-all choice and its copies, each claimed by `<KEY>-<reply id>`,
and it prints `{sent, refused}`: the new attempt of every reply sent, and
`{reply_id, code, message}` for each not sent. A reply whose message has
been answered since by a reply that ended `sent` is refused with
`OUTBOUND_RESEND_INVALID`, so nobody is answered twice, and so is one that
carried files. A listing narrowed by no mailbox, text, file name or window
is `OUTBOUND_RESEND_INVALID` before anything is read; one keeping no failed
reply is `NOT_FOUND`; a blank key is `IDEMPOTENCY_KEY_INVALID`.

## Refusals

| Code | Cause | What to do |
|---|---|---|
| `NOT_FOUND` | `message was not found` | The ID names no message of this organization; take it from `skrzynka message list`. |
| `NOT_FOUND` | `outbound message was not found` | The ID names no outbound message of this organization; take it from `skrzynka message outbound` or the `send` result. |
| `MESSAGE_FILTER_INVALID` | `the to filter is empty; …` (likewise `contains`, `attachment`, `status`), `the status filter is not a delivery status (…)`, `the since filter … is neither an RFC 3339 instant … nor a YYYY-MM-DD day …`, or `since … is not before until …` | Name text to match, or leave the filter out; a status is `pending`, `sending`, `sent`, `failed` or `uncertain`; a window's start comes before its end. |
| usage error, exit 2 | `--id` with `--mailbox`, a filter, `--limit` or `--offset`, or an ID that is not a UUID; `--resend-failed` without `--idempotency-key` or with `--csv` or `--follow-up` | `--id` reads one message; the other flags narrow and page the listing. Use one or the other. |
| `OUTBOUND_RESEND_INVALID` | `narrow the sent mail by mailbox, addressee, text, file name or window first; …` (for replies `narrow the replies by mailbox, text, file name or window first; …`), or, for one message, `outbound message <ID> carried files whose bytes Skrzynka does not keep; …`, `reply <ID> carried files …` or `message <ID> has been answered since by a reply that was sent; …` | Narrow the listing; start a draft with `draft again`, or answer again with the files, for one that carried files. |

## Proof

`tests/messages/reads.mjs` runs the real CLI against a dedicated mailbox that
has replied to synced mail and originated mail. It checks that every message's
replies belong to it and are newest first, that at least one message has a
reply, that `replies --mailbox` lists exactly as many attempts as the
messages hold, newest first, and `--oldest-first` the same attempts turned
round, that `--csv` writes one row per listed attempt in order with its
message and status and refuses the same path again with
`REPLY_OUTPUT_EXISTS`, that `--status` keeps exactly those in that
state and an unknown status is `MESSAGE_FILTER_INVALID`, that `--since` and
`--until` at the newest attempt's claim instant split the attempts there and
a backwards window is refused, that `--contains` a word of the newest
attempt's text, in upper case, keeps exactly the attempts holding it and a
blank one is refused, that `--attachment` a file name a reply carried (or
one nothing carries), in upper case, keeps exactly the attempts carrying
it, that `outbound --id` returns exactly the row the listing holds, that
`--oldest-first` lists exactly the same rows turned round, that `--csv`
writes that header and one row per listed message with its ID first, in
order, and refuses an existing path with `OUTBOUND_OUTPUT_EXISTS`, that
`--to`, `--contains`, `--attachment` (a file name the rows carried, or one
nothing carries), `--status`, `--since` and `--until` (at the newest row's
claim instant), `--bounced` (the rows whose Message-ID a stored report
returned) and `--answered` / `--unanswered` (the rows whose Message-ID the
mailbox's stored mail does or does not name) keep exactly the listed rows they describe
and a text nothing holds keeps none, that a blank filter, an unknown status
and a window whose start is not before its end
are refused with `MESSAGE_FILTER_INVALID`, that an
unknown ID is refused with `NOT_FOUND` for both reads, and that `--id` with
`--mailbox` or a filter and a malformed ID are refused before dispatch; and
that `replies --resend-failed` narrowed by nothing is
`OUTBOUND_RESEND_INVALID`, a blank key `IDEMPOTENCY_KEY_INVALID`, a text no
reply holds `NOT_FOUND`, the flag without a key refused before dispatch, and
that none of those sent a reply. It writes its
commands, exit statuses and observations to
`.build/real-tests/message-reads/run-*/report.json`. `tests/writing/follow-ups.mjs`
checks that `outbound --resend-failed` over a listing holding only a sent message
is `NOT_FOUND` and sends nothing, that a listing narrowed by nothing is
`OUTBOUND_RESEND_INVALID`, a blank key `IDEMPOTENCY_KEY_INVALID`, and the
flag without a key refused before dispatch.

```bash
node tests/messages/reads.mjs --organization ORGANIZATION --mailbox MAILBOX_ID \
  --binary target/debug/skrzynka --source-revision HEAD
```
