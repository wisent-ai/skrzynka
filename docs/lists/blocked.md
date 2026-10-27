# Blocked senders

Blocking a sender leaves its mail out of every message listing. Nothing is
deleted, the provider is not told, and unblocking shows the mail again.

| Verb | CLI | HTTP |
| --- | --- | --- |
| Block | `skrzynka message block <ADDRESS or @DOMAIN>` | `POST /v1/blocked` with `{address}` |
| Unblock | `skrzynka message unblock <ADDRESS or @DOMAIN>` | `DELETE /v1/blocked/:address` (URL-encoded) |
| List | `skrzynka message blocked [--count] [--contains <TEXT>]` | `GET /v1/blocked[?count=true&contains=TEXT]` |
| Import | `skrzynka message blocked --import <FILE>` | `POST /v1/blocked/import` with `{text}` |
| Export | `skrzynka message blocked --export <FILE> [--contains <TEXT>]` | `GET /v1/blocked/export[?contains=TEXT]`, `text/plain` offered as `blocked.txt` |
| Table | `skrzynka message blocked --csv <FILE> [--contains <TEXT>]` | `GET /v1/blocked/csv[?contains=TEXT]`, `text/csv` offered as `blocked.csv` |

Skrzynka Desktop: **Block** and **Unblock** beside each contact in
**Contacts** (inbox inspector, no message selected), blocked domains listed
there with **Unblock** and how many messages each hides (narrowed, like the
contacts, by **Name or address contains**), and **Include blocked** in the
inbox filter bar; **Import blocked senders…** under the contacts imports a
text file and **Export blocked senders…** saves the list as one.

Every form answers `{blocked: [...]}`, every entry of the organization in
address order. An entry is a whole address (`news@example.com`) or `@` and a
domain (`@example.com`) for every address of exactly that domain; it is
stored trimmed and in lower case. Blocking an entry already blocked is no
change. `message blocked --count` answers instead `[{entry, messages}]`, each
entry with how many stored messages of the organization it leaves out of
listings now; a message an address and its domain both block counts under
each. `--contains <TEXT>` lists only the entries holding the text, case
aside, such as a domain, with or without `--count`; a blank text is
`MESSAGE_FILTER_INVALID`.

## Importing a list

`--import <FILE>` takes a UTF-8 text file, such as a spam list exported
from another mail program: one address or `@domain` per line, anything
after it on the line being a note that is not kept; blank lines and lines
starting `#` are skipped. Every line is read before any entry is blocked,
so one unreadable line is `BLOCKED_SENDER_INVALID` quoting it and nothing
changes. It answers `{imported, blocked}`: the entries the file named, once
each in line order and lower case, and every blocked entry after it. An
entry already blocked stays blocked.

`--export <FILE>` writes every blocked entry, or those `--contains` keeps,
to a new file in exactly the form `--import` reads: a `#` line saying what
the file is, then one entry per line in address order. It prints `{path,
entries, size_bytes}`. An existing path is `BLOCKED_OUTPUT_EXISTS` before
anything is read, and a path that cannot be written `BLOCKED_OUTPUT_INVALID`,
the part written removed. `--export` with `--count` or `--import` is refused
by Clap with exit 2.

`--csv <FILE>` (Desktop: **Blocked senders as a table (.csv)…** in
**Contacts**) writes the same entries, each with how many stored messages
it leaves out of listings now, to a new file as a CSV table (RFC 4180, CRLF
lines) under the header `entry, messages`: the rows `--count` prints, for a
spreadsheet or a review of what blocking hides. It prints `{path, entries,
size_bytes}` and is refused as `--export` is; `--csv` with `--count`,
`--export` or `--import` is refused by Clap.

## What it leaves out

`message list` (`GET /v1/messages`) leaves out every message whose From
address is a blocked address or ends with a blocked `@domain`, before the
other filters and the page. `--include-blocked` (`include_blocked=true`)
lists them as well. Contacts, threads, `message show` and every other read
are unchanged, and nothing about receiving or syncing changes.

## Refusals

| Code | When |
| --- | --- |
| `BLOCKED_SENDER_INVALID` | The entry is neither an address nor `@domain` (it has no `@`, more than one, a space or a comma); for `--import`, `the import line "<LINE>": …; nothing was imported`, or a file with no entry; or the file could not be read as UTF-8 text. |
| `BLOCKED_SENDER_NOT_FOUND` (404) | `unblock` names an entry that is not blocked; the message lists the blocked entries. |
| `BLOCKED_OUTPUT_EXISTS`, `BLOCKED_OUTPUT_INVALID` | `--export` or `--csv` names a path that exists, or one that cannot be checked, created or written, with the cause. |

## Real test

`tests/lists/blocked.mjs` runs the built CLI against a dedicated real mailbox
holding stored mail. It blocks the newest message's sender, checks
`message list` leaves out exactly that sender's mail while
`--include-blocked` lists it, checks `blocked --count` counts every message
of the organization from that sender, checks `--contains` the sender in
upper case keeps exactly the entries holding it, with and without
`--count`, and a blank one is refused, blocks and unblocks that sender's
`@domain`, unblocks the address and checks the listing is whole again, and
checks the two refusals; imports a file of a comment, a blank line, a fresh
address in upper case with a note, a fresh `@domain` and the address again,
and checks exactly the two entries are imported in lower case and blocked,
then checks a file whose second line is no address is refused and its first
line not blocked; exports the list and checks the file holds exactly its
entries under the `#` line, that exporting to the same path is
`BLOCKED_OUTPUT_EXISTS`, and that `--export --contains` the imported
`@domain` writes that entry alone; then that `--csv --contains` the
`@domain` writes exactly the rows `--count` lists and the same path again
is `BLOCKED_OUTPUT_EXISTS`. It restores the block list it found.

```sh
node tests/lists/blocked.mjs --organization <ORG> --mailbox <ID> \
  --binary <PATH> --source-revision <COMMIT>
```

Reports remain under `.build/real-tests/blocked-senders/<run>/report.json`.
