# Follow-ups

A follow-up says you expect an answer to a message you sent by an instant.
Skrzynka then tells you which sent messages are owed an answer. It never
sends anything on its own: what to do about a silence is your decision.

| Surface | Set | List | Show | Clear |
| --- | --- | --- | --- | --- |
| CLI | `message followup set <OUTBOUND_ID> --by <INSTANT>` | `message followup list [--state waiting\|due\|answered] [--mailbox <ID or address>] [--to <TEXT>] [--contains <TEXT>] [--due-by <WHEN>]` | `message followup show <OUTBOUND_ID>` | `message followup clear <OUTBOUND_ID>`, or `clear --state <STATE> [--mailbox] [--to] [--contains] [--due-by]` |
| HTTP | `PUT /v1/outbound/:id/follow-up` with `{by}` | `GET /v1/follow-ups[?state=&mailbox_id=&to=&contains=&due_by=]` | `GET /v1/outbound/:id/follow-up` | `DELETE /v1/outbound/:id/follow-up`, or `DELETE /v1/follow-ups?state=…` with the listing's parameters |
| Desktop | Outbound inspector → **Follow-up** → **Set follow-up** | **Follow-ups** (no outbound message selected), with **Standing**, **Mailbox**, **Due within**, **Recipient contains** and **Subject or body contains** / **Find** | the outbound inspector's **Follow-up** section | **Clear**, or **Stop expecting these…** in **Follow-ups** with a **Standing** chosen |

Only an outbound message that ended `sent` can be followed up: its
Message-ID is what an answer names. `--by` is an RFC 3339 instant, or a span
from now as snoozes take it (`+3d`, `+1w`; m, h, d or w; Desktop: **Follow up
in**), stored in UTC; setting it again moves it. `list --state <STATE>`
keeps the follow-ups standing so (see below): `due` those owed an answer
now, `waiting` those whose instant is still ahead, `answered` those a
stored message answers (Desktop's **Standing**). `list --mailbox` keeps the follow-ups of
messages that one mailbox sent, and `list --to` those whose recipients
contain a text, case aside: what is owed by one person or company.
`list --contains <TEXT>` keeps those whose sent message's subject or body
contains a text, case aside: what is owed on one offer or project.
`list --due-by <WHEN>` keeps those expecting an answer before a bound: a
span from now such as `+3d` lists what comes due in the next three days
(Desktop's **Due within**: three days or a week), and an RFC 3339 instant,
a `YYYY-MM-DD` day or a span back such as `-1d` are read as a listing's
`--until` reads them.

`list … --csv <PATH>` (HTTP `GET /v1/follow-ups/csv` with the listing's
parameters, `text/csv` offered as `follow-ups.csv`; Desktop: **Export as a
table (.csv)…** in **Follow-ups**) writes the follow-ups listed, earliest
due first, to a new file as a CSV table (RFC 4180, CRLF lines) under the
header `outbound_id, mailbox_id, recipients, subject, sent_at, due_at,
state, answer, answered_at`, an instant or answer it does not hold empty,
and prints `{path, follow_ups, size_bytes}`: who owes an answer, to work
through in a spreadsheet or CRM. An existing path is
`FOLLOW_UP_OUTPUT_EXISTS` before anything is read, and a path that cannot
be checked, created or written is `FOLLOW_UP_OUTPUT_INVALID`.

`clear --state <STATE>` (HTTP `DELETE /v1/follow-ups?state=…`, role
`member`; Desktop: **Stop expecting these…** in **Follow-ups** once a
**Standing** is chosen, after a confirmation) stops expecting an answer to
every follow-up standing so, narrowed by `--mailbox`, `--to`, `--contains`
and `--due-by` exactly as `list` narrows, and prints them as they stood:
`clear --state answered` tidies away every follow-up that got its answer.
The sent messages are not changed. A standing must be named: an HTTP
request without `state` is `FOLLOW_UP_INVALID`, and the CLI without an ID
or `--state` is refused by Clap, so nothing clears every follow-up at once.

## Expecting an answer as you send

`message send`, `message forward` and `message draft send` take
`--follow-up <BY>` (HTTP: `follow_up` beside `idempotency_key` on
`POST /v1/mailboxes/:id/outbound`, `POST /v1/messages/:id/forward` and
`POST /v1/drafts/:id/send`; Desktop: **Expect an answer in** on Compose,
Forward and the draft editor, named in the send confirmation). `<BY>` is
read as `--by` is, before anything is claimed, so an unreadable one sends
nothing (`FOLLOW_UP_INVALID`; a forward refuses it before reading the
original). Once the send ends `sent`, the follow-up is set exactly as
`followup set` would set it; a send that ends `failed` or `uncertain` gets
none, since nothing reached anyone to answer. A repeated idempotency key
answers its first attempt and leaves its follow-up as it stands. If the
message was sent but the follow-up could not be stored, the refusal names
the outbound message and the `followup set` command that sets it; nothing
is sent again. A scheduled draft is sent without one.

## Expecting answers to many sent messages

`skrzynka message outbound <the listing's flags> --follow-up <BY>` (HTTP
`POST /v1/outbound/follow-ups` with the listing's parameters and `{by}`,
role `member`; Desktop: **Answers expected within** and **Expect answers to
these…** in **Find sent mail**, after a confirmation) expects an answer by
`<BY>`, read as `--by` is, to every message the outbound listing keeps that
ended `sent`, replacing an earlier instant: the way to follow up a mail
merge sent from scheduled drafts, such as `message outbound --contains
<campaign text> --follow-up +5d`. It prints `{follow_ups, skipped}`: each
follow-up set, as `followup list` prints it, and each listed message that
did not end `sent`, as `{outbound_id, status}`, since nothing reached
anyone to answer it. An unreadable `<BY>` is `FOLLOW_UP_INVALID` before
anything is read; a listing narrowed by no mailbox, addressee, text, file
name, window or bounce is `FOLLOW_UP_INVALID`, so nothing expects an answer
to all sent mail at once; one keeping no message is `NOT_FOUND`.
`--follow-up` with `--csv` is refused by Clap.

## Where a follow-up stands

Each listing reads the stored mail of the sending mailbox now. A follow-up
is:

- `answered` when a stored message names the sent Message-ID in its
  `In-Reply-To` or `References` header; `answer` is the first such message
  and `answered_at` when it was received. A bounce report is not an answer;
  `mailbox delivery` lists bounces;
- `due` when nothing answers it and its instant has passed;
- `waiting` when nothing answers it yet and its instant is still ahead.

An answer counts once Skrzynka has imported it: a running service imports as
the provider announces new mail, and `skrzynka sync` imports now.

## Reminding those who have not answered

`skrzynka message followup nudge --template <NAME> [--state waiting|due]
[--mailbox <ID>] [--to <TEXT>] [--contains <TEXT>] [--due-by <WHEN>]`
(HTTP `POST /v1/follow-ups/nudge` with the listing's parameters and
`{"template": NAME}`, role `member`; Desktop: **Reminder template** and
**Draft reminders…** in **Follow-ups**, after a confirmation) drafts one
reminder per follow-up the filters keep that nothing answers yet: from the
mailbox that sent it, to its To and Cc, holding the template's text with
`{{subject}}`, `{{recipients}}`, `{{sent_at}}` and `{{due_at}}` filled from
the sent message and the follow-up where the template names them. Its
subject is the template's when it has one, else the sent subject after
`Re: `. It prints the drafts kept. Nothing is sent: the reminders wait under
`draft list` to be read, scheduled (`draft schedule --contains`) or sent.

Every reminder is filled and every recipient checked before any is kept:
a template naming another placeholder is the template's own refusal
(`TEMPLATE_VALUE_MISSING`) naming the follow-up, and a recipient a send
would refuse — on the suppression list, or bounced for good — is
`FOLLOW_UP_INVALID` naming each such address. Answered follow-ups are owed
nothing and are left out; `--state answered`, or filters keeping none owed
an answer, are `FOLLOW_UP_INVALID`. None of these keeps a draft.

## Refusals

| Code | When |
| --- | --- |
| `FOLLOW_UP_INVALID` | `--by`, or `--follow-up` on a send, is neither an RFC 3339 instant nor a span such as `+3d` (an unknown unit such as `+2y` included), or the outbound message did not end `sent`. A send refused this way claims nothing. `DELETE /v1/follow-ups` without `state`. `nudge` with `--state answered`, filters keeping no follow-up owed an answer, or a reminder to an address a send would refuse; nothing is drafted. |
| `TEMPLATE_NOT_FOUND`, `TEMPLATE_VALUE_MISSING` | `nudge --template` names no template, or one naming a placeholder the sent message and follow-up do not fill; nothing is drafted. |
| `NOT_FOUND` | The organization holds no outbound message with that ID. |
| `FOLLOW_UP_NOT_FOUND` | `show` or `clear` names an outbound message that has no follow-up. |
| `NOT_FOUND`, `MAILBOX_SELECTOR_AMBIGUOUS` | `list --mailbox` names no mailbox of the organization, or an address more than one has. |
| `MESSAGE_FILTER_INVALID` | `list --to` or `list --contains` holds no visible text, or `list --due-by` is neither a span from now, an RFC 3339 instant, a day nor a span back. |
| `FOLLOW_UP_OUTPUT_EXISTS`, `FOLLOW_UP_OUTPUT_INVALID` | `list --csv` names a path that exists, or one that cannot be checked, created or written. |

A `--state` other than `waiting`, `due` or `answered` is refused by Clap
with exit 2; over HTTP an unknown `state` is a 400 naming the three.

## Real test

`tests/writing/follow-ups.mjs` runs the built CLI against a dedicated real
mailbox. It sends one message from it to the mailbox's own address, so the
answer can be written without anyone else. It checks a send with an
unreadable `--follow-up` is refused with `FOLLOW_UP_INVALID` and claims no
row, sends with `--follow-up +1d` and checks the follow-up is `waiting`
with an instant at least a day ahead, checks an unreadable `--by`
and a span with an unknown unit are
refused with `FOLLOW_UP_INVALID` and an unknown outbound message with
`NOT_FOUND`; sets a follow-up `+1d` and checks it is `waiting` with an
instant at least a day ahead, that `--due-by +2d` lists it and `--due-by
+1h` does not, and that an unreadable `--due-by` is refused; sets it at the present instant and checks it is `due`
and listed by `--state due` alone of the three standings and by `--mailbox` with the mailbox (which lists no
other mailbox's), and that an unknown `--mailbox` is `NOT_FOUND`, that
`--to` the recipient in upper case lists it and only follow-ups to that
address and a blank `--to` is `MESSAGE_FILTER_INVALID`, that `--contains`
the subject in upper case lists it, a text nothing sent holds lists none
and a blank `--contains` is `MESSAGE_FILTER_INVALID`, that `--csv` writes
the listed follow-ups in order, this one's row `due` with its subject, and
refuses the same path again with `FOLLOW_UP_OUTPUT_EXISTS`, leaving the
file as it was; saves a template named for the run holding `{{subject}}`
and `{{due_at}}` and checks `nudge --template <it> --state due --contains
<the subject>` drafts exactly one reminder from the mailbox to the
recipients, subject `Re: <subject>`, with the subject and due instant
filled, that `--state answered` and a `--contains` nothing sent holds are
`FOLLOW_UP_INVALID`, that the template saved again naming a placeholder
nothing fills is `TEMPLATE_VALUE_MISSING`, and that the refused nudges kept
no draft; checks `message outbound --contains <the subject> --follow-up
+1d` sets exactly this follow-up, `waiting` a day ahead, with nothing
skipped, that `--follow-up` on a listing narrowed by nothing and an
unreadable `--follow-up` are `FOLLOW_UP_INVALID`, a `--contains` nothing
sent holds `NOT_FOUND`, and `--follow-up` with `--csv` is refused, then
sets it due again;
syncs the mailbox, named by its own address (`sync --mailbox <address>`,
each pass checked to read that mailbox), until the copy arrives, replies to it from the
same mailbox with `--quote` and checks the reply quotes every line of the
copy after `> ` and names its sender (the quoted reply of
[reply-all.md](reply-all.md)), syncs until the reply arrives, and checks the follow-up is
`answered` by that reply and listed by `--state answered`, no longer by
`--state due`, that `show` reads it exactly as the listing holds it, and
that an unknown `--state` is refused; tags the copy for the run, saves a
view listing it alone and checks `message reply --view` sends exactly one
reply to it claimed by `<KEY>-<message id>`, that the same command again
answers the same attempt and that an unknown view is `VIEW_NOT_FOUND` (the
view and tag are removed at the end); then checks `clear` answers it, a
second `clear` is refused with `FOLLOW_UP_NOT_FOUND`, sets the follow-up
again and checks `clear --state due --contains <its subject>` clears none
and `clear --state answered --contains <its subject>` exactly it, that
`clear` with neither an ID nor `--state` is refused, that `show` is then
`FOLLOW_UP_NOT_FOUND`, and `show` of an unknown outbound message is
`NOT_FOUND`. A failed run clears the follow-up; every run deletes its
reminder drafts and its template.

```sh
node tests/writing/follow-ups.mjs --organization <ORG> --mailbox <ID> \
  --binary <PATH> --source-revision <COMMIT>
```

Reports remain under `.build/real-tests/follow-ups/<run>/report.json`.
