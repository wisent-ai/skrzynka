# Contacts

`skrzynka message contacts [--mailbox <ID>] [--recent]` (HTTP `GET /v1/contacts`,
optional `mailbox_id` and `recent=true`; Skrzynka Desktop: **Contacts** in the inbox inspector
while no message is selected, **Mailbox** choosing one) lists everyone the organization's mail is
exchanged with, most exchanged first. `--recent` (Desktop's **Latest
first**) lists them the latest exchanged with first instead: by the later of
`last_received_at` and `last_written_at`. Both orders apply to `--vcard` too.

| Field | Meaning |
| --- | --- |
| `address` | The address, in lower case. |
| `names` | Every display name the address was written with, as written. |
| `received` | Stored messages whose From header names the address. |
| `last_received_at` | When the latest of them was received. |
| `written` | Originated messages (`message send`, `message forward`) not recorded `failed` whose To, Cc or Bcc names the address. |
| `last_written_at` | When the latest of them was claimed. |
| `bounced` | Whether a stored bounce report, in any mailbox of the organization, says delivery to the address failed for good: `skrzynka message bounced` lists it and a new send to it is refused with `OUTBOUND_RECIPIENT_BOUNCED` (see `docs/delivery/report.md`). Desktop marks such a contact **Bounced**. |
| `suppressed` | Whether an entry of `skrzynka message suppressed`, the address or its `@domain`, covers it: no originated mail may be sent to it (`OUTBOUND_RECIPIENT_SUPPRESSED`, see `docs/outreach/suppression.md`). Desktop marks such a contact **Do not send**. |
| `blocked` | Whether an entry of `skrzynka message blocked`, the address or its `@domain`, covers it: listings leave its mail out (see `docs/lists/blocked.md`). Desktop marks such a contact **Blocked**. |

The list is derived from the stored messages and outbound rows on every
read; nothing is kept apart from the mail, so it cannot disagree with it.
Replies are not counted as written: a reply goes to the message's Reply-To or
From, which is already counted as received. Desktop's **Show mail** narrows
the inbox to messages from the address.

## Finding a contact

`--contains <TEXT>` (HTTP `contains=`; Desktop: the **Name or address
contains** field and **Find** above the contacts) keeps only the contacts
whose address, or a name it was written with, contains the text, case
aside, in the same order. It narrows `--vcard` the same way. A blank text is
refused with `MESSAGE_FILTER_INVALID`; nothing matching is `[]`.

`--bounced` (HTTP `bounced=true`; Desktop: **Bounced only**) keeps only the
contacts whose `bounced` is set: the addresses a stored bounce report says
delivery to failed for good, to take out of an address book or correct. It
narrows `--vcard` the same way and combines with `--contains`.
`--suppressed` (HTTP `suppressed=true`; Desktop: **Do not send only**) keeps
only the contacts whose `suppressed` is set, the same way, and `--blocked`
(HTTP `blocked=true`; Desktop: **Blocked only**) those whose `blocked` is
set, to review whom listings leave out.
`--never-written` (HTTP `never_written=true`; Desktop: **One way** →
**Wrote, never answered**) keeps only those who wrote (`received`) and were
never written to (`written` none): who waits on you. `--never-received`
(`never_received=true`; **Written to, never wrote back**) keeps those
written to who never wrote: whom outreach has not reached. The two are
refused together, by Clap with exit 2 and over HTTP with
`MESSAGE_FILTER_INVALID`.

`--since <WHEN>` (HTTP `since=`; Desktop: **Exchanged since** with a date)
keeps only the contacts whose latest exchange, the later of
`last_received_at` and `last_written_at`, is at or after an RFC 3339 instant
or a `YYYY-MM-DD` day read as its UTC start: who you are in touch with now.
An unreadable one is `MESSAGE_FILTER_INVALID`.

`--until <WHEN>` (HTTP `until=`; Desktop: **Quiet since** with a date)
keeps only the contacts whose latest exchange is before the instant or day:
who went quiet, to write to again or take out of an address book. With
`--since` the latest exchange must fall in the window; a window whose start
is not before its end, or an unreadable instant, is `MESSAGE_FILTER_INVALID`.

## As vCards

`skrzynka message contacts [--mailbox <ID>] [--contains <TEXT>] --vcard <PATH>` (HTTP `GET
/v1/contacts/vcard`, typed `text/vcard`; Desktop: **Export as vCards
(.vcf)…**) writes the same correspondents, in the same order, as vCards
(RFC 6350, version 4.0) that any address book imports: `FN` is the first
name the address was written with, or the address when it was written with
none, and `EMAIL` is the address. The file must not exist yet
(`CONTACTS_OUTPUT_EXISTS`); a path that cannot be checked or written is
`CONTACTS_OUTPUT_INVALID`. The command prints `{path, contacts,
size_bytes}`.

## As a table

`skrzynka message contacts [the same flags] --csv <PATH>` (HTTP `GET
/v1/contacts/csv`, typed `text/csv` and offered as `contacts.csv`; Desktop:
**Export as a table (.csv)…**) writes the same correspondents, in the same
order, as a CSV table (RFC 4180) a spreadsheet or a CRM imports: the header
`address,name,received,last_received_at,written,last_written_at,bounced,suppressed,blocked`,
then one row per contact, `name` the first name the address was written
with or empty, an instant empty when there is none, the marks `true` or
`false`. Lines end in CRLF; a field holding a comma, a quote or a line break
is quoted with its quotes doubled. The same refusals and output as
`--vcard` apply; `--vcard` and `--csv` together are refused by the argument
parser.

## Not sending to them again

`skrzynka message contacts [the same flags] --suppress-listed [--reason
<TEXT>]` (HTTP `POST /v1/contacts/suppress` with the listing's parameters
and `{reason}`, role `member`; Desktop: **Why** and **Do not send to
these…** under **Contacts**, after a confirmation) puts the address of every
contact the listing keeps that the [suppression
list](../outreach/suppression.md) does not cover yet on it, with the reason
when given, and prints `{entries}`, each as `message suppressed` lists it:
everyone written to who never wrote back and went quiet (`--never-received
--until -180d`), or everyone of one domain. Each entry is checked before any
is kept. A listing narrowed by no text, window, one-way filter, bounce or
block is `SUPPRESSION_INVALID` before anything is read, so nothing
suppresses everyone a mailbox corresponds with at once (a mailbox alone does
not narrow it); so is a reason with no visible text. A listing keeping no
contact the list does not cover yet is `NOT_FOUND`. `--suppress-listed`
beside `--vcard`, `--csv` or `--suppressed`, and `--reason` without it, are
refused by the argument parser. `message suppressed --remove <ENTRY>` takes
an entry off again.

## Leaving their mail out

`skrzynka message contacts [the same flags] --block-listed` (HTTP `POST
/v1/contacts/block` with the listing's parameters, role `member`; Desktop:
**Block these…** under **Contacts**, after a confirmation) blocks the
address of every contact the listing keeps that no [blocked
entry](../lists/blocked.md) covers yet, so listings leave their mail out, and
prints `{blocked}`, the addresses blocked in the listing's order: such as
everyone of a domain that only sends noise who never wrote back to
(`--contains @news.example.com --never-written`). Nothing is deleted and
the provider is not told. Each entry is checked before any is kept. A
listing narrowed by no text, window, one-way filter, bounce or block is
`BLOCKED_SENDER_INVALID` before anything is read, so nothing hides all of a
mailbox's mail at once; a listing keeping no contact not blocked yet is
`NOT_FOUND`. `--block-listed` beside `--vcard`, `--csv`, `--suppress-listed`
or `--blocked` is refused by the argument parser. `message unblock
<ADDRESS>` shows a sender's mail again.

## Real test

`tests/conversations/contacts.mjs` runs the built CLI against a dedicated
real mailbox. It checks that every sender of the stored mail is listed, that
each `received` count equals the stored messages from that address, that
every `written` count equals the outbound rows not recorded `failed` naming
the address, that `bounced` is set exactly for the addresses `message
bounced` lists, that `suppressed` is set exactly for the addresses an entry
of `message suppressed` covers, that `blocked` is set exactly for those an
entry of `message blocked` covers, and `--bounced`, `--suppressed` and
`--blocked` keep
exactly the contacts flagged so, in the same order, that `--never-written`
and `--never-received` keep exactly the contacts exchanged with that one way,
and that the list is ordered most exchanged first; that
`--recent` lists the same contacts ordered by their latest exchange, newest
first; that `--since` the latest contact's latest exchange keeps exactly the
contacts exchanged with at or after it, in order, and an unreadable `--since`
is `MESSAGE_FILTER_INVALID`; that `--until` the same instant keeps exactly
the contacts last exchanged with before it, in order, and that a window
whose start equals its end is `MESSAGE_FILTER_INVALID`; and that
`--vcard` writes one card per listed contact, each with its address as
`EMAIL`, and refuses an existing path with `CONTACTS_OUTPUT_EXISTS`; that
`--csv` writes that header and one row per listed contact with its address,
in order, and that `--vcard` beside `--csv` is refused; that
`--contains` with the first contact's domain in upper case keeps exactly the
contacts whose address or a name contains it, in order; and that a blank
`--contains` is refused with `MESSAGE_FILTER_INVALID`; that `--contains`
one unsuppressed contact's address with `--suppress-listed --reason` adds
exactly the unsuppressed contacts that listing keeps, with that reason,
flags them suppressed and finds none left the second time (`NOT_FOUND`),
after which the entries are removed and the list is as before; and that an
unnarrowed `--suppress-listed` and a blank `--reason` are
`SUPPRESSION_INVALID`; and that `--block-listed` over one unblocked
contact's address blocks exactly the unblocked contacts that listing keeps,
flags them blocked and finds none left the second time (`NOT_FOUND`), after
which they are unblocked and the blocked list is as before, and that an
unnarrowed `--block-listed` is `BLOCKED_SENDER_INVALID`.

```sh
node tests/conversations/contacts.mjs --organization <ORG> --mailbox <ID> \
  --binary <PATH> --source-revision <COMMIT>
```

Reports remain under `.build/real-tests/message-contacts/<run>/report.json`.
