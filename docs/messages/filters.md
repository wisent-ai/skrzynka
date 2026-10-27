# Finding stored mail

Skrzynka keeps every message its mailboxes received. To read the ones sent to
one address, such as a sign-up confirmation a provider sent to a generated
address, or every message that mentions a word or arrived in a period, narrow
the listing by recipient, sender, text and receive time. The same filters
exist on every surface:

| Surface | Form |
|---|---|
| CLI | `skrzynka message list --recipient ADDRESS --cc ADDRESS --sender TEXT --contains TEXT --since WHEN --until WHEN --tag TAG --list TEXT --attachment TEXT --with-attachments --files-over BYTES --bounces --unanswered --answered --failed-authentication --snoozed --oldest-first` |
| API | `GET /v1/messages?recipient=ADDRESS&cc=ADDRESS&sender=TEXT&text=TEXT&since=WHEN&until=WHEN&tag=TAG&list=TEXT&attachment=TEXT&with_attachments=true&files_over=BYTES&bounces=true&unanswered=true&answered=true&failed_authentication=true&snoozed=true&oldest_first=true` |
| Desktop | Inbox, the fields and toggles above the table, then Apply |

The CLI calls the text filter `--contains` because the global `--text` chooses
the output format. Every filter also takes `--mailbox` (`mailbox_id`),
`--limit` and `--offset`. `--mailbox` takes the mailbox's ID or its own
address, here, on every read that narrows to one mailbox (`message
replies`, `subscriptions`, `contacts`, `tags`, `attachment find`, `draft
list`, `followup list`, `outbound`, `rule list`), on `message export` and
`message restore`, and on `skrzynka sync`; an unknown one is `NOT_FOUND` and an
address more than one mailbox has `MAILBOX_SELECTOR_AMBIGUOUS`, while over
HTTP `mailbox_id` takes the ID. Listings are newest first; `--oldest-first`
(`oldest_first=true`; Desktop: **Oldest first**) lists what the filters keep
oldest first instead, and pages are cut in that order. A saved view keeps
the order with its filters.

## What matches

- **Recipient** matches an address in the To header whole, without case. An
  entry written as a display name with the address in angle brackets matches
  its bracketed address. A longer address that merely ends in the same text is
  a different recipient and does not match.
- **Cc** (`--cc`, `cc=`; Desktop: **Cc address**) matches an address in the
  Cc header whole, the same way: the mail you were copied on. A message that
  states no Cc, or was imported before Cc was recorded, matches no address.
- **Sender** matches any part of the From header, without case, so a
  provider's sending domain is enough.
- **Text** (`--contains`, `text=`) matches any part of the Subject header,
  the stored text or your [note](../lists/notes.md), without case. It reads
  what Skrzynka stored, not the provider's copy: text inside an attachment
  does not match.
- **Since** keeps messages Skrzynka received at or after the instant;
  **until** keeps those received before it. Each is an RFC 3339 instant
  (`2025-01-31T09:00:00+01:00`), a `YYYY-MM-DD` day, read as the start of
  that day in UTC, or a span back from now: `-` and a whole number of
  minutes, hours, days or weeks (`-30m`, `-12h`, `-7d`, `-2w`), read when the
  listing is. A saved view holding `-7d` therefore always lists the last
  seven days. The receive instant is when the import stored the message,
  not the `Date` header the sender wrote. Desktop's **Received** menu fills
  **Since** with `-1d`, `-7d` or `-30d`.
- Every other `--since` and `--until` takes the same three forms: outbound
  mail, replies, contacts, addresses that bounced, a mailbox's delivery and
  summary, and an export, which reads the span once and records the
  instant it stood for in its manifest.
- **Tag** keeps messages carrying one of your tags (`message tag`).
- **List** keeps messages whose `List-Id` header contains the text, without
  case: the mail of one mailing list.
- **Attachment** (`--attachment`, `attachment=`; Desktop: **Attachment name**)
  keeps messages carrying a file whose recorded name contains the text,
  without case, such as `invoice` or `.pdf`. A file that states no name is
  not found by it.
- **With attachments** keeps messages the import recorded at least one
  attachment of. A message imported before attachments were recorded is not
  known to carry any and is not kept; `message attachment list` records it.
- **Files over** (`--files-over BYTES`, `files_over=BYTES`; Desktop: **Files
  over (bytes)**) keeps messages whose recorded files come to at least that
  many bytes together: the mail that takes the room. A file whose size was
  not recorded counts as none; a value that is not a whole number of bytes is
  refused before anything is read.
- **Bounces** keeps only delivery status notifications (`bounce` is set).
- **Unanswered** (`--unanswered`, `unanswered=true`; Desktop: **Unanswered**)
  keeps messages no reply to has been sent for: none of their reply attempts
  ended `sent`. A failed or uncertain reply answers nothing, and a message
  answered from another mail client, outside Skrzynka, still counts as
  unanswered here. A view may keep it like any other filter. **Answered**
  (`--answered`, `answered=true`; Desktop: **Answered**) keeps the others:
  messages at least one reply to ended `sent`. The two exclude each other:
  the CLI refuses the pair before dispatch, HTTP with `MESSAGE_FILTER_INVALID`.
- **Failed authentication** (`--failed-authentication`,
  `failed_authentication=true`; Desktop: **Failed authentication**) keeps
  messages whose receiving provider recorded at least one
  [authentication verdict](../delivery/authentication.md) other than `pass`
  (`fail`, `softfail`, `neutral`, `none`, an error). A clause that is not
  `method=result`, and a message with no recorded verdicts, say nothing
  either way and are not kept.
- Mail of blocked senders (`message block`, `docs/lists/blocked.md`) is left
  out of every listing; `--include-blocked` (`include_blocked=true`) keeps it.
- With any filter, `--limit` and `--offset` page the matching messages, not
  the whole inbox: the second match is `--limit 1 --offset 1`.
- No match is an empty list, `[]`, not an error.

## As a table

`message list … --csv PATH` writes the messages the listing keeps — by the
same filters, saved view, order and page — to a new file as a CSV table (RFC
4180, CRLF lines, a field holding a comma, quote or line break quoted) and
prints `{path, messages, size_bytes}`. The header is `id, mailbox_id,
received_at, sent_at, sender, reply_to, recipients, cc, subject, message_id,
in_reply_to, list_id, snippet, attachments, tags, note, snoozed_until`; a
header the message did not carry is empty, its text is left out but for the
snippet, `attachments` names each recorded file (an unnamed one by its type)
and `tags` each tag, `; ` between them. `GET /v1/messages/csv` with the
listing's parameters answers the same bytes as `text/csv` offered as
`messages.csv`; in Skrzynka Desktop, **Export as a table (.csv)…** in the
inbox filter bar saves the messages the applied filter lists, in the
selected mailbox's scope. The file is never overwritten: an existing path is
`MESSAGE_OUTPUT_EXISTS` before anything is read, and a path that cannot be
checked, created or written is `MESSAGE_OUTPUT_INVALID` with the cause. The
inbound feed of a declared inbound domain's address is not written as a
table (`MESSAGE_FILTER_INVALID`, as for its other filters).

## Refusals

| Code | Cause | What to do |
|---|---|---|
| `MESSAGE_FILTER_INVALID` | `the recipient filter is empty; name what it must match or leave the filter out` (or `sender`, `text`, `tag`, `list`) | Name a value, or drop the flag. A blank filter is never read as no filter. |
| `MESSAGE_FILTER_INVALID` | `the since filter "<VALUE>" is neither an RFC 3339 instant (<CAUSE>), a YYYY-MM-DD day (<CAUSE>) nor a span back from now such as -7d` (or `until`), or `-<SPAN> is not a span from now: <WHY>; write - a whole number and m, h, d or w, such as -2h` | Write an instant, a day or a span. |
| `MESSAGE_FILTER_INVALID` | `since <INSTANT> is not before until <INSTANT>; no message could be received between them` | Put `--since` before `--until`. |
| `MESSAGE_FILTER_INVALID` | `<ADDRESS> is an address of a declared inbound domain, whose feed is read by recipient and sender only; leave the other filters out` | The CLI reads an inbound domain's feed for such an address; keep only `--recipient` and `--sender`. |
| `LIMIT_INVALID` | `--limit 0` | Omit `--limit` to list every match. |

## Example

A purchase journey signed an account up with a generated address and waits for
the provider's code:

```bash
skrzynka sync
skrzynka message list --recipient ADDRESS --sender anthropic.com --limit 1
```

The newest matching message is first; its `body_text` holds the code.

## Saved views

`message list … --save-view <NAME>` keeps the filters given, and
`--mailbox`, under a name, replacing a view of the same name, then lists as
usual; `message list --view <NAME>` (`GET /v1/messages?view=NAME`) lists
what the view keeps. Any other filter beside `--view` is refused with
`MESSAGE_FILTER_INVALID`, so a listing never mixes a view with filters the
reader cannot see. `message view list|show|rename|copy|delete` (`GET /v1/views`, `GET` /
`PUT` / `DELETE /v1/views/:name`, `POST /v1/views/:name/rename` and
`POST /v1/views/:name/copy` with `{to}`) manage them; Skrzynka Desktop's
filter bar offers **Views** to open one, **Save view** to keep the filters
as they stand, and **Rename view**, **Copy view** and **Delete view**.
`message view copy <NAME> --to <NEW>` keeps a view's filters and mailbox
under a second name too and answers the copy, leaving the view as it was,
so one can be changed without the other. `message view list --count` (`GET /v1/views?count=true`)
answers each view with `messages`, how many stored messages it lists now,
counted as `message list --view` lists them; Desktop's **Views** menu shows
that count beside each name. `--contains <TEXT>` (`contains=`; Desktop's
**Find view**, which narrows every views menu) lists only the views whose
name holds the text, case aside; a blank one is `MESSAGE_FILTER_INVALID`.
A view keeps the filters as written and is checked as a listing
checks them when saved and each time it lists: a view that names no filter,
or a bad name, is `VIEW_INVALID`; an unknown one is `VIEW_NOT_FOUND`, naming
the views there are; renaming or copying onto a name another view carries
is `VIEW_EXISTS`. Renaming keeps the filters. `tests/messages/filter.mjs`
covers them, renaming, copying (the copy holds the same filters and the
view is unchanged, and renaming or copying onto it is refused) and `view
list --count --contains` (its name in upper case lists it alone, its count
equals the messages `--view` lists, and a blank text is refused) included.

`message view export --output <FILE> [--contains <TEXT>]` (`GET
/v1/view-document[?contains=]`, offered as `views.json`; Desktop's
**Export views…** in the filter bar, every view) writes every view, or
those whose name holds the text, to a new file as JSON `[{name, mailbox,
filters}]` in name order and prints `{path, views, size_bytes}`: `mailbox`
is the address of the mailbox a view keeps to, which another installation
resolves, or `null`, and `filters` are the view's filters with
`mailbox_id` left `null`. An existing path is `VIEW_OUTPUT_EXISTS` (one
that cannot be checked or written `VIEW_OUTPUT_INVALID`) before anything is
read. `message view import --input <FILE>` (`POST /v1/view-document` with
`{text}`; Desktop's **Import views…**) keeps every view of such a file,
replacing one of the same name as `--save-view` does, and prints `{views}`
in file order. The whole file is read, every `mailbox` resolved by ID or
address and every view checked as `--save-view` checks one before any is
kept: a file that is not such a list, holds none, names one view twice,
sets `filters.mailbox_id` or holds a view naming no filter is
`VIEW_INVALID`, an unknown mailbox `NOT_FOUND`, and nothing changes.
`tests/views/transfer.mjs` saves a view kept to the mailbox, exports it
and checks the file holds exactly its name, the mailbox's address and its
filters, that exporting to the same path is `VIEW_OUTPUT_EXISTS`, deletes
the view, imports the file and checks the view reads back with the same
filters, and checks each refusal above keeps nothing; its report is under
`.build/real-tests/view-transfer/run-*/report.json`:

```bash
node tests/views/transfer.mjs --organization ORGANIZATION --mailbox MAILBOX_ID \
  --binary target/debug/skrzynka --source-revision HEAD
```

## Proof

`tests/messages/filter.mjs` runs the real CLI against a dedicated mailbox that
already holds synced mail. It checks that the recipient filter returns exactly
the messages addressed to the newest message's recipient, in order and without
case; that a longer address does not match; that `--cc` with an address a
message was copied on, in upper case, keeps exactly the messages whose Cc
names it (an address nobody was copied on when no message states a Cc); that the sender filter keeps the
message; that the text filter keeps it by a word of its subject and drops it
for text it does not hold; that `--oldest-first` lists the mailbox and the
recipient's mail turned round and cuts a one-message page to the oldest; that `--with-attachments`, `--bounces`,
`--attachment`, `--files-over` (the largest stored file's size), `--unanswered`, `--answered` and `--failed-authentication` keep exactly the stored messages their
field or reply attempts mark; that a page is cut after the filter; and that a
blank recipient, sender, text, tag, list or attachment is refused with `MESSAGE_FILTER_INVALID`;
and that `--since -<N>m`, N minutes reaching back before the mailbox's
oldest stored message, lists exactly what the listing without it lists,
while `--since -7x` is refused with `MESSAGE_FILTER_INVALID`. It
writes its commands, exit statuses and observations to
`.build/real-tests/message-filter/run-*/report.json`.

```bash
node tests/messages/filter.mjs --organization ORGANIZATION --mailbox MAILBOX_ID \
  --binary target/debug/skrzynka --source-revision HEAD
```

`tests/tables/messages.mjs` writes the same mailbox's listing with `--csv`
and reads the table back as a spreadsheet would (`tests/tables/csv.mjs`):
it checks the header, that the rows are exactly the listed messages in
order with each field as the listing prints it, that `--since` with
`--oldest-first` narrows and orders the table as the listing, that a text
nothing holds writes the header alone, that an existing path is
`MESSAGE_OUTPUT_EXISTS` and left unchanged, a path under a missing
directory `MESSAGE_OUTPUT_INVALID` and not created, and a blank filter
`MESSAGE_FILTER_INVALID`. Its report, with each table's SHA-256, is
`.build/real-tests/message-table/run-*/report.json`.

```bash
node tests/tables/messages.mjs --organization ORGANIZATION --mailbox MAILBOX_ID \
  --binary target/debug/skrzynka --source-revision HEAD
```
