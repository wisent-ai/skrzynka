# Exporting a mailbox

`skrzynka message export --mailbox <ID> --output <DIRECTORY>` (Skrzynka
Desktop: **Export mailbox…** in the mailbox inspector) saves every stored
message of one mailbox as its original into a new directory:

- the directory is created; an existing path is refused with
  `EXPORT_OUTPUT_EXISTS` before anything is read, so an export never mixes
  with or overwrites earlier files;
- each stored message, blocked senders', snoozed and muted mail included, is read
  from the provider by UID with `BODY.PEEK[]` (nothing is marked read) and
  written as `<message id>.eml`, the whole MIME source as the provider holds
  it, which any mail program opens;
- `manifest.json` lists `exported` files with `size_bytes` and `sha256`, and
  `refused` messages with the refusal's own code and sentence, such as
  `MESSAGE_SOURCE_GONE` for one the provider no longer holds. The command
  prints the same object, with the `view`, `thread`, `since`, `until` and `tag` it was narrowed by or `null`.

`--mbox` (Desktop: **Also one mailbox.mbox for a mail program**) also
writes `mailbox.mbox`, every exported original in the manifest's order in
the mboxrd form Apple Mail, Thunderbird and mutt import: each after a
`From MAILER-DAEMON <asctime of its receive instant, UTC>` line, its lines
ending in LF, a line reading `From ` after any number of `>` given one
more `>`, and a blank line after it. The manifest names it as `mbox`,
`{file, size_bytes, sha256, messages}`; `null` without the flag. The
`.eml` files are written either way, so `message restore` reads the same
directory.

`--view <NAME>` (Desktop: **Only a view's messages**) exports only the
mailbox's messages a [saved view](../messages/filters.md) lists, as
`message list --view` lists them; a view kept to another mailbox exports
none. An unknown view is `VIEW_NOT_FOUND` before the directory is created.

`--thread <MESSAGE_ID>` (Desktop: **Export conversation…** in the message
inspector's Conversation section) exports only the conversation that
message belongs to, as [`message thread`](../conversations/thread.md) lists
it. The message must be the mailbox's own; one of another mailbox is
refused with `EXPORT_THREAD_ELSEWHERE`, naming its mailbox. `--view` and
`--thread` exclude each other.

`--tag <TAG>` (Desktop: **Only messages tagged**) exports only the
mailbox's messages carrying the tag, as `message list --tag` keeps them,
without saving a view first; it combines with `--since` and `--until` and
excludes `--view` and `--thread`. `--mailbox` takes the mailbox's ID or its
own address.

`--since <WHEN>` exports only the messages received at or after an RFC 3339
instant, a `YYYY-MM-DD` day read as its UTC start, or a span back from now
such as `-7d`, as `message list --since` keeps them: what arrived since the
last export, kept beside it. `--until <WHEN>` (Desktop: **Only before**)
exports only those received before it, so `--since` and `--until` together
export one period, such as a month. A span is read once, when the export
starts, and the manifest names the instant it stood for, never the span.
Both exclude `--view` and `--thread`. An unreadable instant, or a `since`
not before `until`, is `MESSAGE_FILTER_INVALID`, before the directory is
created.

A message that cannot be read does not stop the others. When any was
refused, the command ends with `EXPORT_INCOMPLETE` (exit 1) after printing
and writing the manifest, so a script cannot mistake a partial backup for a
whole one. Desktop reports the same sentence. [`message verify`](verify.md)
later checks the directory still holds what was written.

Skrzynka keeps only what each import records, not the whole original, so an
export reads the provider. A message the provider deleted or moved out of
INBOX after it was imported is listed under `refused`.

An export goes back to the provider with [`message restore`](restore.md),
which checks each file against this manifest and appends it to a folder
that does not hold it yet.

## Refusals

| Code | When |
| --- | --- |
| `NOT_FOUND` | The organization holds no mailbox with that ID. |
| `VIEW_NOT_FOUND` | `--view` names no saved view. |
| `NOT_FOUND` (message) | `--thread` names no message of the organization. |
| `EXPORT_THREAD_ELSEWHERE` | `--thread` names a message of another mailbox; the refusal names it. |
| `MESSAGE_FILTER_INVALID` | `--since` or `--until` is neither an RFC 3339 instant, a `YYYY-MM-DD` day nor a span back from now such as `-7d`, or `since` is not before `until`. |
| `EXPORT_OUTPUT_EXISTS` | `--output` already exists. |
| `EXPORT_OUTPUT_INVALID` | The directory, a message file or the manifest could not be created or written; the cause is named. |
| `EXPORT_INCOMPLETE` | At least one message could not be exported; `refused` names each and why. |

## Real test

`tests/backup/export.mjs` runs the built CLI against a dedicated real
mailbox holding stored mail. It exports into a new directory under the run's
report folder and checks that every message `message list
--include-blocked --include-snoozed --include-muted` lists for the mailbox is either a file
whose SHA-256 matches the manifest and whose bytes match `message source`
for that message, or a `refused` entry with a code; that the printed object
is the manifest; that exporting into the same directory again is
refused with `EXPORT_OUTPUT_EXISTS`; that `--view` of a freshly drawn tag on
one message exports that message alone and names the view in the manifest,
that `--tag` of the same tag, the mailbox named by its address, exports it
alone and names the tag;
that `--thread --mbox` of that message accounts for exactly the messages
`message thread` lists and names the message as `thread`, and that its
`mailbox.mbox` is byte for byte the exported originals written as mboxrd,
each dated with its receive instant, as the manifest's `mbox` describes it;
that `--since` the newest message's receive instant accounts for exactly the
messages `message list --since` keeps and names the instant as `since`, that
`--until` the same instant accounts for exactly the messages `message list
--until` keeps and names it as `until`, that `--since -1m` names in the
manifest the instant a minute before the export, read once, and
an unreadable `--since` is `MESSAGE_FILTER_INVALID`;
and that an unknown view is `VIEW_NOT_FOUND`. It removes the tag and view.

```sh
node tests/backup/export.mjs --organization <ORG> --mailbox <ID> \
  --binary <PATH> --source-revision <COMMIT>
```

Reports remain under `.build/real-tests/export/<run>/report.json`.
