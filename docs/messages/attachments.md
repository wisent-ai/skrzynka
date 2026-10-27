# Reading a message's attachments

Skrzynka records, when it imports a message, which of its MIME parts are
files: each one's index (counted from zero, in MIME order), the name the
message gives it, its type and its decoded size. It keeps no attachment bytes.
Saving an attachment reads the message again from its mailbox's INBOX by UID
with `BODY.PEEK[]`, so nothing is marked seen at the provider, and returns the
bytes of that one part.

| Surface | List | Save |
|---|---|---|
| CLI | `skrzynka message attachment list MESSAGE_ID` | `skrzynka message attachment save MESSAGE_ID --index N --output PATH`, or `--all --output DIRECTORY` |
| API | `GET /v1/messages/:id/attachments` | `GET /v1/messages/:id/attachments/:index` |
| Desktop | Inbox, the message inspector's Attachments section | **Save…** beside an attachment; **Save all to a new folder…** |

## What counts as an attachment

A part that is not a multipart container and whose `Content-Disposition` is
`attachment`, or which states a file name (`filename` or `name`, RFC 2231 and
RFC 2047 encodings read), or which is a whole message (`message/rfc822`). The
same message always yields the same indexes, so an index recorded at import
names the same part when the message is read again.

## The `attachments` field

Every stored message carries `attachments`:

- an array, possibly empty, of `{index, filename, content_type, size_bytes,
  unreadable}`: what the import recorded. `filename` is null when the part
  names none; `size_bytes` is null and `unreadable` says why when the part's
  transfer encoding cannot be decoded (or its stated name cannot be read);
- `null` for a message imported before attachments were recorded. That is not
  "no attachments": listing them reads the message from the provider once and
  records what it carries, after which the field holds the array.

## Saving

`attachment save` refuses a path that already exists before it contacts the
provider, writes the bytes to a file it creates, and prints `message_id`,
`index`, `filename`, `content_type`, `size_bytes`, `sha256` and `path`. A file
that cannot be written whole is removed. The API answers the bytes with the
part's `Content-Type` and `Content-Disposition: attachment` carrying the name
as an ASCII `filename` and the exact UTF-8 `filename*`.

`attachment save MESSAGE_ID --all --output DIRECTORY` (Desktop: **Save all
to a new folder…**) creates the directory, refusing one that exists before
the provider is read, and saves every attachment into it as
`<index>-<name>` (`<index>-attachment` for a file that states no name); the
name is cut to its last path component, so nothing is written outside the
directory. It prints `{message_id, directory, saved, refused}`: each file
saved as one `save` prints it, and each attachment that could not be read
or written with its message, index, and its refusal's code and sentence.
One refused does not stop the others; when any was, it ends with
`ATTACHMENT_SAVE_INCOMPLETE` (exit 1) after printing. `--all` and `--index`
exclude each other.

Before answering, Skrzynka checks that the provider's copy under that UID is
still the message it stored (UID, headers and text) and that the attachment
is described as it was recorded.

## Refusals

| Code | HTTP | Sentence | What to do |
|---|---|---|---|
| `NOT_FOUND` | 404 | `message was not found` | Use an ID `message list` prints for this organization. |
| `ATTACHMENT_INDEX_INVALID` | 400 | `attachment index "X" is not a whole number counted from zero: …` (API only; the CLI refuses a non-number before running) | Use an index `attachment list` prints. |
| `ATTACHMENT_NOT_FOUND` | 404 | `message M has no attachment N: it has K, indexed A to B` or `…: it has no attachments` | Use one of the indexes named. |
| `ATTACHMENT_UNDECODABLE` | 502 | `attachment N of message M, "name" (type, size unknown), cannot be read: its transfer encoding could not be decoded: …` | The sender's part is damaged; open the message in the provider's own client. |
| `MESSAGE_SOURCE_GONE` | 410 | `INBOX of ADDRESS at HOST:PORT no longer holds UID U, where message M was imported from: the provider deleted, archived or moved it; its stored text remains, nothing more can be read from it` | Find the message where the provider moved it. |
| `MESSAGE_SOURCE_CHANGED` | 409 | `UID U in INBOX of ADDRESS at HOST:PORT now holds a different message than message M (stored Message-ID …, provider Message-ID …); the provider renumbered its mail, so nothing was read by this UID`, or `attachment N of message M was recorded as … but the provider's copy now describes it as …; nothing was read` | The provider renumbered INBOX; nothing local is wrong, and the attachment is no longer reachable by this UID. |
| `ATTACHMENT_OUTPUT_EXISTS` | CLI | `PATH already exists; saving never overwrites a file: name a path that does not exist` | Name a new path. |
| `ATTACHMENT_OUTPUT_INVALID` | CLI | `PATH could not be checked: …`, `PATH could not be created: …`, or `PATH could not be written: …; the partial file was removed` | Fix the directory or its permissions. |
| `ATTACHMENT_SAVE_INCOMPLETE` | CLI | `N of the files could not be saved; each is listed under refused` | Read each refusal; the others were saved. |

IMAP connection, authentication and fetch failures (`IMAP_UNAVAILABLE`,
`IMAP_AUTHENTICATION_FAILED`, `IMAP_INBOX_UNAVAILABLE`, `IMAP_FETCH_FAILED`)
and Skarbiec credential refusals are the same as for synchronization.

## Finding files across messages

`skrzynka message attachment find [--mailbox ID] [--name TEXT] [--type TEXT]
[--over BYTES] [--sender TEXT] [--since WHEN] [--until WHEN] [--tag TAG]
[--largest-first]` (HTTP `GET
/v1/attachments?mailbox_id=&name=&type=&over=&sender=&since=&until=&tag=&largest_first=true`; Desktop: **Files**
in the inbox inspector while no message is selected, **Mailbox** choosing one) lists every file the
stored mail records, of every mailbox or one, blocked senders', snoozed and
muted mail included, newest message first and in each message's own order:
`{message_id, mailbox_id, sender, subject, received_at, index, filename,
content_type, size_bytes}`, `index` being what `attachment save --index`
takes. `--name` keeps files whose recorded name contains the text, `--type`
those whose content type contains it (`pdf`, `image/`), both case aside;
`--over` those of at least that many recorded bytes; `--sender` those of
mail whose From header contains the text; `--since` and `--until` those of
mail received at or after and before an RFC 3339 instant, a `YYYY-MM-DD`
day or a span back from now such as `-30d` (Desktop's **Received since** and **Received before**);
`--tag` those of mail carrying the tag, as `message list --tag` keeps it
(Desktop's **Tagged**). `--largest-first` lists the files kept largest first,
those of no recorded size last and files of one size newest first, to find
what takes the room (Desktop's **Largest first**).
A file whose name or size was not
recorded passes no name or size filter, and a message imported before its
files were recorded names none until `attachment list` records them. Nothing
is read from a provider. A blank text, an unreadable instant or a window
whose start is not before its end is `MESSAGE_FILTER_INVALID`.

`attachment find … --csv PATH` (HTTP `GET /v1/attachments/csv` with the
same parameters, `text/csv` offered as `attachments.csv`; Desktop:
**Export as a table (.csv)…** in **Files**) writes the files found, in
order, to a new file as a CSV table (RFC 4180, CRLF lines) under the header
`message_id, mailbox_id, sender, subject, received_at, index, filename,
content_type, size_bytes`, a name, subject or size not recorded empty, and
prints `{path, files, size_bytes}`: an audit of which mail carried which
file. An existing path is `ATTACHMENT_OUTPUT_EXISTS` before anything is
read, and a path that cannot be checked, created or written is
`ATTACHMENT_OUTPUT_INVALID`.

`attachment find … --save DIRECTORY` (Desktop: **Save every file listed to
a new folder…** in **Files**, which reads each through `GET
/v1/messages/:id/attachments/:index`) reads every file found from its
provider, as `attachment save` reads one, and writes it into a new
directory as `<message id>-<index>-<name>`, so files of different messages
never meet: every invoice of a sender, or every PDF of the last month, at
once. It prints `{directory, saved, refused}` as `--all` does. Filters
keeping no file are `NOT_FOUND` (`attachment was not found`) before the
directory is created; an existing directory is `ATTACHMENT_OUTPUT_EXISTS`;
a file that cannot be read or written is listed under `refused` and the
command ends with `ATTACHMENT_SAVE_INCOMPLETE` after printing. `--save` and
`--csv` exclude each other.

## Proof

`tests/messages/attachments.mjs` runs the real CLI against a dedicated
mailbox whose synced mail includes at least one message with an attachment.
It lists the attachments, saves one, compares the file's size and SHA-256 with
what the command printed and what the listing recorded, and checks the
refusals for an index past the last attachment and for an existing output
path. `--all` must account for every attachment once, each saved file inside
the new directory matching its printed digest and the chosen one matching
the single save; an existing directory is refused, and `--all` with
`--index` is refused before running. `attachment find` must list exactly
the files the mailbox's stored messages record, in order, and `--type`,
`--over` and `--name` (from the chosen file, written in upper case) keep
exactly the files they describe; a name no file holds keeps none and a blank
one is `MESSAGE_FILTER_INVALID`; `--since` and `--until` at the chosen
message's receive instant split the files between them and a backwards
window is refused; `--largest-first` must list the same files with sizes
falling, and `--tag` a freshly drawn tag put on the chosen message must keep
exactly that message's files (the tag is removed afterwards); `--csv` must
write exactly those files, each row the file as `find` prints it, and refuse
the same path again with `ATTACHMENT_OUTPUT_EXISTS`, leaving the file as it
was; `--save` with the tag must write exactly that message's files into a
new directory, each matching its printed digest, refuse the same directory
again with `ATTACHMENT_OUTPUT_EXISTS`, and a name no file holds be
`NOT_FOUND`. It never sends, deletes or moves mail.

```bash
node tests/messages/attachments.mjs --organization ORGANIZATION --mailbox MAILBOX_ID \
  --binary target/debug/skrzynka --source-revision HEAD
```
