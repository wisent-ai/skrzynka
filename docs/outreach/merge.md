# Mail merge

One template, one CSV table, one draft per row: the drafts are read before
anything goes out, then scheduled at a steady pace, or removed together when
the merge was a mistake. Nothing here sends mail by itself; a scheduled
draft is sent by a running `skrzynka serve`, as `docs/writing/drafts.md`
describes.

| Verb | CLI | HTTP | Desktop (**Drafts** in the inbox inspector) |
| --- | --- | --- | --- |
| Merge | `skrzynka message draft save --mailbox <ID or address> --template <NAME> --merge <TABLE.csv>` | `POST /v1/drafts/merge` with `{mailbox, template, table}` | **Merge template** and **Merge a table (.csv)…** |
| Schedule them | `skrzynka message draft schedule --contains <TEXT> [--mailbox <ID or address>] --at <INSTANT> [--every <SPAN>] --idempotency-key <KEY>` | `POST /v1/drafts/schedule` with `{contains, mailbox_id, send_at, every, idempotency_key}` | **First at**, **Apart** and **Schedule the drafts containing it** with a text in **Contains** |
| Remove them | `skrzynka message draft delete --contains <TEXT> [--mailbox <ID or address>]` | `DELETE /v1/drafts?contains=&mailbox_id=` | **Delete the drafts containing it…** with a text in **Contains** |

Every change needs role `member`.

## Merging

`draft save --template <NAME> --merge <TABLE.csv>` keeps one draft per row
of a CSV table as a spreadsheet writes it (RFC 4180: quoted fields may hold
commas, doubled quotes and line breaks; blank lines are skipped), from the
mailbox `--mailbox` names (Desktop: the one chosen under **Mailbox**, or the
selected one). The row's `to` column, one address or several separated by
commas, addresses the draft, and every other column, named by the header
without case, fills the template's `{{key}}` placeholder of that name in
its subject and text. It prints the drafts kept, in the table's order. A
text the template carries in every draft, such as a campaign's name, is
what `--contains` finds them by below.

Every row is read, addressed and filled before any draft is kept, so one bad
row keeps none, and the refusal names the row by its `to`:

| Code | When |
| --- | --- |
| `DRAFT_MERGE_INVALID` | The table cannot be read as UTF-8 or holds a quote never closed, is empty or holds a header and no row, names no `to` column or a column twice, or has a row of another length than the header or with no `to` address; or a recipient a send would refuse, named with why: one the suppression list covers (naming the entry) or a stored bounce report says failed for good, as `message recipients` reads them, so the table is corrected before anything is drafted. |
| `OUTBOUND_RECIPIENT_INVALID` | A row's `to` holds an address that does not read. |
| `TEMPLATE_VALUE_UNUSED`, `TEMPLATE_VALUE_MISSING` | A column the template does not name, or a placeholder no column fills. |
| `TEMPLATE_NOT_FOUND`, `NOT_FOUND`, `MAILBOX_SELECTOR_AMBIGUOUS` | The template or the mailbox is not the organization's, or the address names more than one mailbox. |

`--merge` needs `--template` and takes no `--to`, `--cc`, `--bcc`,
`--subject`, `--set` or `--attach`; the argument parser refuses otherwise.

## Scheduling them at a steady pace

`draft schedule --contains <TEXT> --at <INSTANT> [--every <SPAN>]
--idempotency-key <KEY>` schedules every draft mentioning the text that is
not scheduled yet, the earliest written first: the first at `--at` and each
next `--every` after the one before (`5m`, `1h`, `1d`; without it all at
`--at`; Desktop's **Apart**: all at once, 5 or 15 minutes, an hour), so a
mailbox sends at a pace receiving providers trust. Each is scheduled exactly
as one `draft schedule <ID>` would be, held to the suppression list and the
bounce reports, and claimed by `<KEY>-<draft id>`, so the same command again
claims the same sends. It prints the drafts scheduled; Desktop says how many
and when the last goes out, and keeps its key until the scheduling succeeds.

A span that does not read, an unreadable instant, or none waiting to be
scheduled is `DRAFT_SCHEDULE_INVALID` before anything is scheduled, and a
blank key or one holding whitespace `IDEMPOTENCY_KEY_INVALID`. A draft one
schedule refuses (`OUTBOUND_RECIPIENT_BOUNCED`,
`OUTBOUND_RECIPIENT_SUPPRESSED`, an instant already passed) unschedules the
ones scheduled before it and ends with that refusal, saying so.

`draft unschedule --contains <TEXT> [--mailbox <ID or address>]` (HTTP
`DELETE /v1/drafts/schedule?contains=`; Desktop's **Unschedule the drafts
containing it**) holds a staggered merge back: it takes every scheduled
draft mentioning the text off its schedule and prints them, still drafts,
to be corrected and scheduled again. Without a text it is
`MESSAGE_FILTER_INVALID` over HTTP and refused by the argument parser on the
CLI; a text no scheduled draft holds is `NOT_FOUND`.

## Following up who did not answer

A scheduled draft is sent without a follow-up. Once the merge has gone out,
`message outbound --contains <TEXT> --follow-up +5d` expects an answer to
every sent message mentioning the text (Desktop: **Expect answers to
these…** in **Find sent mail**), and later `message followup nudge
--template <NAME> --state due --contains <TEXT>` drafts a reminder from a
template to each recipient who has not answered, ready to be scheduled the
same way; see [follow-ups](../writing/follow-ups.md).

## Removing a merge made by mistake

`draft delete --contains <TEXT> [--mailbox <ID or address>]` removes every
draft mentioning the text and prints them as they stood; Desktop asks first.
Without a text it is `MESSAGE_FILTER_INVALID` over HTTP and refused by the
argument parser on the CLI, so nothing removes every draft at once; a text
no draft holds is `NOT_FOUND`.

## Real test

`tests/tables/merge.mjs` saves a template naming `{{name}}` and
`{{company}}` in its subject and text, merges a table of two rows (one name
holding a comma and a quote, quoted as a spreadsheet writes it) and checks
the two drafts carry each row's filled subject and text and its `to`, and
that `draft list` holds exactly them; then checks a column the template
does not name is `TEMPLATE_VALUE_UNUSED`, a placeholder no column fills
`TEMPLATE_VALUE_MISSING`, a table without `to`, one with a header and no
row and one with a quote never closed `DRAFT_MERGE_INVALID`, a row with an
unreadable address `OUTBOUND_RECIPIENT_INVALID`, a row addressed inside a
domain it suppressed for the run (`message suppressed --add @<marker>.example`,
removed again before it ends) `DRAFT_MERGE_INVALID`, that none of them kept a
draft, and that `--merge` without `--template` is refused. Then it checks
`draft schedule --contains <its marker> --every <a span that does not
read>` is `DRAFT_SCHEDULE_INVALID` and schedules none, that `--every
<SPAN>` from the operator's `--schedule-at` schedules exactly the merged
drafts in the order written, the first at that instant and each next one an
equal gap later, each claimed by `<KEY>-<draft id>`, and that the same
command again, nothing waiting, is `DRAFT_SCHEDULE_INVALID`. It checks
`draft unschedule --contains <its marker>` takes exactly the merged drafts
off their schedule and keeps them all, and that the same command then is
`NOT_FOUND`. Last it checks
`draft delete --contains <its marker>` removes and answers exactly the
merged drafts, that the same command then is `NOT_FOUND`, and that a delete
naming neither an ID nor a text is refused. It removes whatever is left of
its drafts, and its template, before it ends, before the instant named, so
nothing is sent; its report, with each table's SHA-256, is
`.build/real-tests/draft-merge/run-*/report.json`.

```sh
node tests/tables/merge.mjs --organization <ORG> --mailbox <ID> \
  --recipient <ADDRESS> --schedule-at <INSTANT A DAY AHEAD> --every 5m \
  --binary <PATH> --source-revision <COMMIT>
```
