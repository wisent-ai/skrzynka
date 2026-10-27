# Mailbox summary

`skrzynka mailbox summary <MAILBOX_ID>` (HTTP `GET
/v1/mailboxes/:id/summary`; Skrzynka Desktop: **Summary** in the mailbox
inspector) counts the mail Skrzynka stored for one mailbox, now, from the
stored messages themselves; nothing is kept apart from them and no provider
is contacted.

| Field | Counts |
| --- | --- |
| `messages` | Every stored message, blocked senders', snoozed and muted mail included. |
| `unanswered` | Those no reply was sent to (none of their reply attempts ended `sent`), as `message list --unanswered` keeps them. |
| `snoozed` | Those snoozed until an instant still ahead. |
| `blocked` | Those from a [blocked sender](../lists/blocked.md). |
| `muted` | Those of a [muted conversation](../conversations/thread.md). |
| `tagged` | Those carrying at least one tag. |
| `noted` | Those carrying your note. |
| `with_attachments` | Those the import recorded at least one file of. |
| `file_bytes` | The recorded sizes of every file those carry, added up: how much room the mail's files take. A file whose size was not recorded adds nothing. |
| `bounces` | Delivery status notifications. |
| `failed_authentication` | Those with a sender [authentication verdict](authentication.md) other than `pass`. |
| `senders` | Distinct From headers, without case. |
| `first_received_at`, `last_received_at` | The first and last receive instants; `null` for a mailbox with no stored mail. |
| `days` | `[{day, received}]`: how many of those were received on each UTC day that received any, oldest first, adding up to `messages`; Desktop lists them as **Received per day**. |

Each count is the length of the matching `message list --mailbox <ID>`
listing with the same filter and `--include-blocked --include-snoozed --include-muted`, so
the summary never disagrees with the listings. How the mailbox's sends ended
is [`mailbox delivery`](report.md).

## A window

`--since <WHEN>` and `--until <WHEN>` (HTTP `since=`, `until=`; Desktop:
**Only since** and **Only before**, each with a date) count only the mail received at or after `since`
and before `until`, as `message list --since --until` keeps it; either may be
left out. Each is an instant, a `YYYY-MM-DD` day or a span back from now
such as `-7d`. The summary names them in `since` and `until` as the
instants they stood for when it was counted, `null` for no bound. An
unreadable bound, or `since` not before `until`, is
`MESSAGE_FILTER_INVALID`.

## The days as a table

`mailbox summary <ID> … --days-csv <PATH>` (HTTP `GET
/v1/mailboxes/:id/summary/days/csv` with the same window, `text/csv`;
Desktop: **Received per day as a table (.csv)…** in **Summary**) writes the
summary's `days` to a new file as a CSV table (RFC 4180, CRLF lines) under
the header `day, received`, oldest first, and prints `{path, rows,
size_bytes}`. An existing path is `SUMMARY_OUTPUT_EXISTS` before anything is
read; one that cannot be checked, created or written `SUMMARY_OUTPUT_INVALID`.

## Refusals

| Code | When |
| --- | --- |
| `NOT_FOUND` | The organization holds no mailbox with that ID. |
| `MESSAGE_FILTER_INVALID` | `--since` or `--until` does not read as an instant or day, or the window is backwards. |
| `SUMMARY_OUTPUT_EXISTS`, `SUMMARY_OUTPUT_INVALID` | `--days-csv` names a path that exists, or one that cannot be checked, created or written. |

## Real test

`tests/mailbox/summary.mjs` runs the built CLI against a dedicated real
mailbox. It reads the summary and checks every count against the listing
`message list` gives for the same filter (all stored mail, `--unanswered`,
`--with-attachments`, `--bounces`, `--failed-authentication`, tagged and
noted messages), the senders against the distinct From headers, the first
and last receive instants against the listing, `days` against the listing
counted per UTC receive day, that `--since` the latest
receive instant counts exactly the messages `message list --since` keeps
then and names that instant, that `--until` that instant counts exactly the
messages `message list --until` keeps and that the two windows add up to the
whole mailbox, that `--since -1m` is named as the instant a
minute before the summary, that an unknown mailbox is `NOT_FOUND`, and that
`--days-csv` writes exactly the summary's days in order and refuses the same
path again with `SUMMARY_OUTPUT_EXISTS`.

```sh
node tests/mailbox/summary.mjs --organization <ORG> --mailbox <ID> \
  --binary <PATH> --source-revision <COMMIT>
```

Reports remain under `.build/real-tests/mailbox-summary/<run>/report.json`.
