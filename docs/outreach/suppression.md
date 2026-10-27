# Recipients not to send to

The suppression list holds the recipients no originated mail may be sent
to: a whole address, or `@domain` for every address of a domain, each with
why, such as a person who asked not to be written to again. It belongs to
the organization and lives only in Skrzynka.

| Surface | List | Add | Import | Export | Remove |
| --- | --- | --- | --- | --- | --- |
| CLI | `skrzynka message suppressed [--contains <TEXT>] [--since <WHEN>] [--until <WHEN>]` | `message suppressed --add <ADDRESS or @DOMAIN> [--reason <TEXT>]` | `message suppressed --import <FILE> [--reason <TEXT>]` | `message suppressed --export <FILE> [--contains …] [--since …] [--until …]`, or `--csv <FILE>` for a table | `message suppressed --remove <ENTRY>` |
| HTTP | `GET /v1/suppressed[?contains=TEXT&since=WHEN&until=WHEN]` | `POST /v1/suppressed` with `{entry, reason}` (`reason` optional) | `POST /v1/suppressed/import` with `{text, reason}` (`reason` optional) | `GET /v1/suppressed/export` (same parameters), `text/plain` offered as `suppressed.txt`; `GET /v1/suppressed/csv`, `text/csv` offered as `suppressed.csv` | `DELETE /v1/suppressed/:entry` (URL-encoded) |
| Desktop | **Do not send to** in the Outbound inspector while no message is selected, with **Entry or reason contains**, **Find** and **Added** | an address or `@domain`, **Reason (optional)**, **Add** | **Import a list…** | **Export the list…**, **Export as a table (.csv)…** | **Remove** beside an entry |

Each entry is `{entry, reason, added_at}`, the entry trimmed and in lower
case, `reason` `null` when none was given; the list is in entry order.
Adding an entry already there replaces its reason and keeps when it was
first added. Changes need the `member` role. `--contains <TEXT>` lists only
the entries whose entry or reason contains the text, case aside, such as a
domain or a word of why, to find one in a long imported list; a blank text
is `MESSAGE_FILTER_INVALID`. `--since <WHEN>` and `--until <WHEN>` list
only the entries added (`added_at`) at or after and before an RFC 3339
instant, a `YYYY-MM-DD` day read as its UTC start or a span back from now
such as `-7d`, to review what was suppressed lately (Desktop's **Added**:
the last day, week or 30 days, or before the last 30 days to review the old entries); an unreadable instant, or a window whose
start is not before its end, is `MESSAGE_FILTER_INVALID`.

`--import` takes a UTF-8 text file, such as a do-not-contact list exported
from elsewhere: one address or `@domain` per line, optionally followed by
whitespace and that line's own reason; a line without one takes `--reason`.
Blank lines and lines starting `#` are skipped. Every line is read before
any is kept, so one unreadable entry refuses the whole file with
`SUPPRESSION_INVALID`, quoting the line, and nothing changes. It answers
`{entries}`, every entry added or whose reason was replaced, in line order.
Desktop's **Import a list…** reads the chosen file and hands its text to
the core with the reason typed in **Reason (optional)**.

`--export <FILE>` writes the entries listed, narrowed by `--contains`,
`--since` and `--until` as the listing is, to a new file in exactly the
form `--import` reads: a `#` line saying what the file is, then one entry
per line in entry order with its reason after a space. Importing it
elsewhere, or here after a mistake, gives back the same entries with the
same reasons. It prints `{path, entries, size_bytes}`; an existing path is
refused with `SUPPRESSION_OUTPUT_EXISTS` before anything is read, and a
path that cannot be written with `SUPPRESSION_OUTPUT_INVALID`, removing
what was partly written. Desktop's **Export the list…** saves the same
bytes where you choose. `--csv <FILE>` (`GET /v1/suppressed/csv`,
Desktop's **Export as a table (.csv)…**) writes the same entries instead as
a CSV table (RFC 4180, CRLF lines) under the header `entry, reason,
added_at`, a reason not given empty, for a CRM or mailing tool keeping the
same do-not-contact list; it prints the same object and is refused the same
way. `--csv` and `--export` are not given together.

## What is refused

`message send`, `message forward` and `message draft send` (HTTP `POST
/v1/mailboxes/:id/outbound`, `POST /v1/messages/:id/forward`, `POST
/v1/drafts/:id/send`; Desktop's Compose, Forward and draft editor) naming a
listed address in To, Cc or Bcc, or an address of a listed `@domain`, are
refused with `OUTBOUND_RECIPIENT_SUPPRESSED` (409) before anything is
claimed: no outbound row is written, nothing counts against the send limit
and the provider is not contacted. The sentence names each address with the
entry that holds it and the reason. `message draft schedule` refuses such a
draft the same way when it is scheduled, since no one would be there when
its time came. There is no override: `--allow-bounced` does not reach the
list, because the list is the operator's own word; take the entry off the
list to send. A repeated idempotency key still answers its first attempt.
Replies are not held to the list: answering someone who wrote is not
writing to them unasked.

Addresses a bounce report says failed for good are a separate list,
[`message bounced`](../delivery/report.md), learned from the mail itself.

## Checking before writing

`message recipients <ADDRESS>...` (HTTP `POST /v1/recipients/check`
with `{addresses}`; Desktop: **Check recipients** below the To, Cc and Bcc
fields of Compose, Forward and the draft editor) answers, for each address
in the order given (a value may hold several separated by commas; a repeat
is answered once), `{address, suppressed, bounced, sendable}`: the
suppression entry that covers it, itself or by its domain, what the stored
bounce reports say about it (as `message bounced` lists it), and whether a
send naming it would pass both checks. Nothing is sent, claimed or
counted. An address a send would refuse as unreadable, or naming none, is
`OUTBOUND_RECIPIENT_INVALID` (400).

## Refusals

| Code | HTTP | When |
| --- | --- | --- |
| `SUPPRESSION_INVALID` | 400 | The entry is neither an address nor `@domain`, `--reason` holds no visible text, an imported line is unreadable (quoted), the import holds no entry, or the file cannot be read as UTF-8 text. |
| `SUPPRESSION_NOT_FOUND` | 404 | `--remove` names an entry the list does not hold; the sentence names those it does. |
| `SUPPRESSION_OUTPUT_EXISTS`, `SUPPRESSION_OUTPUT_INVALID` | — | `--export` or `--csv` names a path that exists, or one that cannot be checked, created or written, with the cause. |
| `OUTBOUND_RECIPIENT_SUPPRESSED` | 409 | A send, forward, draft send or schedule names a listed recipient. Nothing is claimed. |
| `OUTBOUND_RECIPIENT_INVALID` | 400 | `message recipients` names an address that does not read as one, or none. |

`--reason` without `--add` or `--import`, two of `--add`, `--import` and
`--remove`, and `--contains`, `--since`, `--until` or `--export` beside any of them,
are refused by Clap with exit 2. A blank `--contains`, an unreadable
instant or a backwards window is `MESSAGE_FILTER_INVALID` (400).

## Real test

`tests/sending/suppression.mjs` runs the built CLI against a dedicated real
mailbox at a freshly drawn domain nobody holds, so nothing could be
delivered. It adds an address written in upper case with spaces and a
reason and checks it is kept trimmed and in lower case with the reason,
adds that address's `@domain` without one, and checks the list holds both
in entry order. It checks `message send` to the address, and to another
address of the domain, is refused with `OUTBOUND_RECIPIENT_SUPPRESSED`
naming the address and claims no row, that the refusal carries the reason
and `--allow-bounced` does not override it, and that a draft to the address
is refused on `draft send` and on `draft schedule` and stays unscheduled,
and that `message recipients` of the address, another address of the domain
and an address of another fresh domain answers the address's own entry,
the domain's entry and a sendable address, in that order, while an
unreadable address is `OUTBOUND_RECIPIENT_INVALID`.
It checks an entry that is no address and a blank reason are
`SUPPRESSION_INVALID` and removing an unknown entry is
`SUPPRESSION_NOT_FOUND`. It imports a file of a comment, a blank line, an
address in upper case with its own reason and one without, and checks
exactly the two are kept in lower case with their own and the shared
reason, then imports a file whose second line is no address and checks it
is refused quoting that line and its first line was not kept, and that
`--contains` the domain in upper case lists exactly the four entries, a
reason lists the one line using it, and a blank one is refused, that
`--since -1d` lists the four and `--until -1d` none of them, and that a
backwards window and an unreadable instant are refused. It then
removes every entry, each answered, and
checks the list no longer holds them; exports the list that remains and
checks the file holds exactly its entries with their reasons under the `#`
line, that exporting to the same path is `SUPPRESSION_OUTPUT_EXISTS`, that
`--csv` writes one row per entry in order with its entry and instant added
under the header `entry,reason,added_at` and refuses the same path again,
and
that importing the file gives back the same entries and reasons. A failed run removes what it added
and deletes its draft.

```sh
node tests/sending/suppression.mjs --organization <ORG> --mailbox <ID> \
  --binary <PATH> --source-revision <COMMIT>
```

Reports remain under `.build/real-tests/suppression/<run>/report.json`.
