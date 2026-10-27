# Drafts

A draft is a message written over time from one mailbox and kept in
Skrzynka until it is sent. Nothing reaches the provider until the send.

| Verb | CLI | HTTP |
| --- | --- | --- |
| Start | `skrzynka message draft save --mailbox <ID or address> [--to <A>]… [--cc <A>]… [--bcc <A>]… [--subject <S>] [--body-file <PATH>] [--attach <TYPE=PATH>]…` | `POST /v1/drafts` with `{mailbox, to, cc, bcc, subject, body, attachments}` |
| Start one per row of a table, schedule or remove them together | `draft save --template <NAME> --merge <TABLE.csv>`, `draft schedule --contains <TEXT> --at <INSTANT> --every <SPAN>`, `draft delete --contains <TEXT>` | `POST /v1/drafts/merge`, `POST /v1/drafts/schedule`, `DELETE /v1/drafts?contains=`; see `docs/outreach/merge.md` |
| Start from a sent message | `skrzynka message draft again <OUTBOUND_ID>` | `POST /v1/outbound/:id/draft` |
| Copy | `skrzynka message draft copy <ID> [--mailbox <ID or address>]` | `POST /v1/drafts/:id/copy` with `{mailbox}` (optional) |
| Change | `skrzynka message draft update <ID> [--to …] [--cc …] [--bcc …] [--subject …] [--body-file …] [--attach … \| --no-attachments]` | `PUT /v1/drafts/:id` with any of `{to, cc, bcc, subject, body, attachments}` |
| List | `skrzynka message draft list [--mailbox <ID or address>] [--scheduled] [--contains <TEXT>] [--bounced] [--suppressed] [--since <WHEN>] [--until <WHEN>] [--with-attachments] [--send-by <WHEN>]` | `GET /v1/drafts[?mailbox_id=&scheduled=true&contains=&bounced=true&suppressed=true&since=&until=&with_attachments=true&send_by=]` |
| List as a table | `skrzynka message draft list [the listing's flags] --csv <PATH>` | `GET /v1/drafts/csv` (the listing's parameters) |
| Read | `skrzynka message draft show <ID>` | `GET /v1/drafts/:id` |
| Remove | `skrzynka message draft delete <ID>` | `DELETE /v1/drafts/:id` (204) |
| Send | `skrzynka message draft send <ID> --idempotency-key <KEY>` | `POST /v1/drafts/:id/send` with `{idempotency_key}` |

Skrzynka Desktop shows **Drafts** in the inbox inspector while no message is
selected: **New draft**, **Open**, and in the editor **Save draft**,
**Send…** (after a confirmation) and **Delete**; its files are listed under
**Kept with the draft**, **Attach file…** adds files that replace them on
save, and **Remove the kept files** saves it with none. **Keep as a new
draft** in the outbound inspector starts one from the selected originated
message. **Mailbox**, **Scheduled only**, **To a bounced address**, **To a suppressed
recipient**, **Untouched for a week**, **Changed this week**, **With files**, **Going out within** and **Contains** with **Find** above
the drafts narrow the list as `--mailbox`, `--scheduled`, `--bounced`, `--suppressed`,
`--until -7d`, `--since -7d`, `--with-attachments`, `--send-by +1d|+1w` and `--contains` do.

A draft is `{id, mailbox_id, recipients, cc, bcc, subject, body, attachments,
created_at, updated_at}`. Every field but the mailbox may be empty while it is written.
A change replaces only the fields it names; `--to`, `--cc` or `--bcc` given at all
replaces every address of that kind. `bcc` holds [blind copies](bcc.md), sent
to and named in no header of the sent message.

`draft list` answers most recently changed first. `--scheduled` keeps the
drafts a running service will send, soonest `send_at` first; `--contains`
those whose To, Cc, Bcc, subject or body contains the text, case aside. A
blank `--contains` is `MESSAGE_FILTER_INVALID`. `--bounced` keeps the drafts
whose To, Cc or Bcc names an address `message bounced` lists: a send of one
is refused with `OUTBOUND_RECIPIENT_BOUNCED`, and a scheduled one is
unscheduled for it, so they are the drafts to correct first. `--suppressed`
keeps the drafts whose To, Cc or Bcc names a recipient an entry of
[`message suppressed`](../outreach/suppression.md) covers: a send or a
schedule of one is refused with `OUTBOUND_RECIPIENT_SUPPRESSED`, with no
override. `--since <WHEN>` and `--until <WHEN>` keep the drafts last changed
(`updated_at`) at or after and before an RFC 3339 instant, a `YYYY-MM-DD`
day read as its UTC start or a span back from now such as `-7d`, so
`--until -7d` lists the drafts left half-written for a week; an unreadable
instant, or a window whose start is not before its end, is
`MESSAGE_FILTER_INVALID`. `--with-attachments` keeps the drafts carrying at
least one file, the ones to check before they go or to lighten.
`--send-by <WHEN>` keeps the scheduled drafts a running service will send
before a bound, soonest first: a span from now such as `+1d` (what goes out
within a day), or an RFC 3339 instant or `YYYY-MM-DD` day as `--until`
reads it; one sent at the bound itself is not kept. An unreadable bound is
`MESSAGE_FILTER_INVALID`.

`draft list … --csv <PATH>` (HTTP `GET /v1/drafts/csv` with the listing's
parameters, `text/csv` offered as `drafts.csv`; Desktop: **Export as a
table (.csv)…** in **Drafts**, for the toggles and pickers set) writes the
drafts the listing keeps, in its order, to a new file as a CSV table (RFC
4180, CRLF lines) under the header `id, mailbox_id, recipients, cc, bcc,
subject, created_at, updated_at, send_at, schedule_error, attachments`,
without their bodies, `attachments` naming each file kept, `; ` between
them, and prints `{path, drafts, size_bytes}`: the plan of what is being
written, to review in a spreadsheet. An existing path is
`DRAFT_OUTPUT_EXISTS` before anything is read; a path that cannot be
checked, created or written is `DRAFT_OUTPUT_INVALID` with the cause.

## Starting from a sent message

`draft again <OUTBOUND_ID>` starts a new draft from an originated message
of any status, `uncertain` and `failed` included: its mailbox, To, Cc, Bcc,
subject and body (the body without the signature, which the send adds
again). The outbound row keeps only the names and digests of the files it
carried, not their bytes, so the draft carries none; attach them again
with `draft update --attach`. The row itself is not changed. Sending the
draft is a new message with a new key, never a resend of the row.

## Copying a draft

`draft copy <ID>` (Desktop: **Copy** beside a draft, **From the same
mailbox** or **From** another) starts a new draft holding the same To, Cc,
Bcc, subject, body and files, and prints it; the original is left as it was.
The copy is written from the draft's own mailbox, or from the mailbox
`--mailbox` names (its ID or address), so one message can go out from
another address. A copy is never scheduled; schedule it on its own.
`NOT_FOUND` names an unknown draft or mailbox, and an address more than one
mailbox has is `MAILBOX_SELECTOR_AMBIGUOUS`.

## Files

`--attach TYPE=PATH` (repeatable; the type is stated, never guessed) reads
each file now and keeps it with the draft until it is sent or deleted;
`attachments` in HTTP takes `[{filename, content_type, content_base64}]`.
Given at all, the files replace every file the draft holds;
`--no-attachments` (`"attachments": []`) removes them, and a change that
names no files keeps them. Each file is checked as `message send` checks it
when it is added, so one that could never be sent is refused then with
`OUTBOUND_ATTACHMENT_INVALID`. A draft prints its files as `attachments:
[{filename, content_type, size_bytes, sha256}]`; their bytes are never
printed. Sending carries them as `message send --attach` does, and the
outbound row records them.

## Sending

`draft send` originates the draft exactly as `message send` does: it is
claimed by its idempotency key, counted against the mailbox's send limit,
signed with the mailbox's signature, held to every rule of `message send`
(a recipient, a subject and a body are required then), and answers the
outbound row. Once the provider accepted it (`sent`) the draft is removed;
any other outcome keeps the draft so it can be corrected and sent again with
a new key.

## Scheduling

`skrzynka message draft schedule <ID> --at <RFC 3339 instant> --idempotency-key <KEY>`
(`POST /v1/drafts/:id/schedule` with `{send_at, idempotency_key}`; Desktop:
**Send at** and **Schedule** in the draft editor) has a running
`skrzynka serve` send the draft at that instant with that key; the draft
shows `send_at` and `send_key`. `draft unschedule <ID>`
(`DELETE /v1/drafts/:id/schedule`; Desktop **Unschedule**) takes it off the
schedule and keeps it.

`skrzynka message send … --at <RFC 3339 instant> --idempotency-key <KEY>`
(Desktop: **Send later** with **Send at** on Compose, its confirmation
**Schedule message**) does both in one step: it keeps the message, its
files included, as a draft of the mailbox and schedules it under the key,
then prints the scheduled draft. A schedule that is refused (an instant
already passed, a recipient that bounced or is suppressed) removes that
draft again, so a refused send later leaves nothing behind; should the
removal itself fail, the refusal names the draft to delete. `--at` excludes
`--allow-bounced` and `--follow-up`, because a scheduled send is always
held to the bounce reports and sets no follow-up.

Every service connected to the organization's fleet database learns of a
schedule change at once, from the database (a `skrzynka_drafts`
announcement), and wakes at the earliest scheduled instant; nothing is
polled. The send is `draft send` with the stored key, so two services, or a
service started again while sending, make one send. Nothing sends a
scheduled draft while no service runs; it is sent when one starts.

A scheduled send that does not end `sent` takes the draft off its schedule
and keeps why in `schedule_error` (`CODE: message`), so it is never tried
again unasked. Correct it, then schedule it again with a new key.

A draft whose To, Cc or Bcc names an address `message bounced` lists is not
scheduled at all: `draft schedule` refuses it with
`OUTBOUND_RECIPIENT_BOUNCED` (409), naming the addresses, because a
scheduled send is always held to the bounce reports and nobody would be
there to override them. Correct the address, clear it with `message bounced
--clear`, or send the draft now with `--allow-bounced`.

## Refusals

| Code | When |
| --- | --- |
| `DRAFT_FILE_INVALID` | `--body-file` could not be read, is not a regular file, or is not UTF-8. |
| `OUTBOUND_ATTACHMENT_INVALID` | An `--attach` value names no type or no readable file, or a file has a bad name, type or base64. Nothing is stored. |
| `NOT_FOUND` | No draft (or, on save, no mailbox; on `again`, no outbound message) with that ID in the organization. |
| `MAILBOX_SELECTOR_AMBIGUOUS` | The address names more than one mailbox; name it by ID. |
| `MESSAGE_FILTER_INVALID` | `draft list --contains` was given a blank text, `--since` or `--until` is unreadable, or the window's start is not before its end. |
| `DRAFT_OUTPUT_EXISTS`, `DRAFT_OUTPUT_INVALID` | `draft list --csv` names a path that exists, or one that cannot be checked, created or written. |
| `DRAFT_MERGE_INVALID` | `draft save --merge` was given a table that cannot be read, is empty, names no `to` column or a column twice, holds no row, or has a row of another length or with no address; nothing was drafted. |
| `IDEMPOTENCY_KEY_REQUIRED` | `draft send` or `draft schedule` was given no `--idempotency-key`. |
| `DRAFT_SCHEDULE_INVALID` | `--at` is not an RFC 3339 instant, or it has already passed. |
| `OUTBOUND_RECIPIENT_BOUNCED` (409) | `draft schedule` of a draft addressed to an address `message bounced` lists; nothing is scheduled. |
| `OUTBOUND_RECIPIENT_SUPPRESSED` (409) | `draft send` or `draft schedule` of a draft addressed to a recipient `message suppressed` covers; nothing is sent or scheduled. |
| `DRAFT_ANNOUNCEMENTS_UNAVAILABLE` | The service could not subscribe to schedule changes; it logs this and stops sending scheduled drafts until it starts again. |
| `OUTBOUND_*`, `MAILBOX_SEND_LIMIT_REACHED`, SMTP outcomes | As for `message send`, at send. |

If the provider accepted the send but the draft could not be removed, the
refusal names the outbound row and the `draft delete` command that removes
it; the message is not sent again.

## Real test

`tests/writing/drafts.mjs` runs the built CLI against a dedicated real
mailbox and a recipient the operator names. It starts a draft carrying a
file, checks the draft lists it by name, size and SHA-256, changes only
its subject and checks the other fields and the file stayed, finds it in `draft list`
and by its subject with `--contains` (and not by a text it lacks, nor with
`--scheduled`; `--bounced` keeps it exactly when its recipient is one
`message bounced` lists, and `--suppressed` leaves it out), checks a blank `--contains` is `MESSAGE_FILTER_INVALID`,
checks `--since` its last change keeps it, `--until` that instant and
`--until -1d` leave it out and a backwards window is refused,
checks `--csv` writes exactly the listed drafts in order, no body column,
and this draft's subject, recipients and file name, and refuses the same
path again with `DRAFT_OUTPUT_EXISTS`, leaving the file as it was,
checks a file without a type is refused, checks a send without a body is
refused and keeps the draft, completes it,
sends it and checks the outbound row is `sent` and records the file, and the draft is gone, and
checks `draft show` of the sent draft is `NOT_FOUND`. A second draft is
copied with `draft copy`, which must hold the same mailbox, addresses,
subject, body and file and leave the original as it was, while an unknown
draft or `--mailbox` is `NOT_FOUND`; the copy is deleted, the second draft
must be listed by `--with-attachments` while it carries its file and not
once `--no-attachments` removed it, and is deleted. `draft again` of the
sent message must give a new draft with its mailbox, recipients, subject
and body and no files, and an unknown outbound ID is `NOT_FOUND`.

```sh
node tests/writing/drafts.mjs --organization <ORG> --mailbox <ID> \
  --recipient <ADDRESS> --binary <PATH> --source-revision <COMMIT>
```

Reports remain under `.build/real-tests/message-drafts/<run>/report.json`.

The mail merge, its scheduling and its removal are proven by
`tests/tables/merge.mjs`, described in `docs/outreach/merge.md`.

`tests/writing/scheduled-drafts.mjs` needs a `skrzynka serve` already running
against the same fleet database. It checks a past instant is refused with
`DRAFT_SCHEDULE_INVALID`, schedules a complete draft the operator-given
`--lead-seconds` ahead, checks it shows `send_at` and `send_key`, then reads
it until the service has sent it or recorded `schedule_error`, and checks the
outbound row with that key is `sent`. It also schedules and unschedules a
second draft and checks it stays, unscheduled; checks `message send --at`
keeps the message, subject and text, as a draft scheduled under its key
(then unscheduled, so it is never sent), that `draft list --send-by` its
send instant leaves it out and a minute later keeps it, that an unreadable
`--send-by` is `MESSAGE_FILTER_INVALID`, and that `send --at` a past instant
is `DRAFT_SCHEDULE_INVALID` and leaves no draft; and, when `message bounced`
lists an address, checks a draft to it is refused with
`OUTBOUND_RECIPIENT_BOUNCED` and left unscheduled. It waits as long as the
service takes; with no service running it does not end.

```sh
node tests/writing/scheduled-drafts.mjs --organization <ORG> --mailbox <ID> \
  --recipient <ADDRESS> --lead-seconds <S> --binary <PATH> --source-revision <COMMIT>
```
