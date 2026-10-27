# Notes on messages

`skrzynka message note <ID> --file <PATH>` (HTTP: `PUT /v1/messages/:id/note`
with `{note}`; Skrzynka Desktop: **Note** → **Save note** in the message
inspector) keeps your own text on a stored message, replacing the note it
had. `--clear` (`{note: null}`; **Remove note**) removes it. `--append`
with `--file` (`{note, append: true}`; Desktop: a line and **Add to note**)
adds the text on a line of its own below the note, or starts the note when
there is none: a running log kept on the message.

Every read of the message (`message show`, `message list`, `GET
/v1/messages`, Desktop's inspector) carries the note in its `note` field,
`null` when there is none. The note is kept with trailing whitespace
trimmed. Notes live only in Skrzynka: nothing changes at the provider, and
deleting the message's mailbox removes them. The text filter (`message list
--contains`, `text=`, Desktop's subject-or-text field) also looks in notes,
so a word you noted finds the message; `--noted` (`noted=true`; Desktop:
**With a note**) lists only the messages carrying a note.

## Refusals

| Code | When |
| --- | --- |
| `MESSAGE_NOTE_INVALID` | The note, or the text `--append` adds, holds no visible text, or `--file` cannot be read as a regular UTF-8 file. |
| `NOT_FOUND` | The organization holds no message with that ID. |

Giving neither or both of `--file` and `--clear`, or `--append` without
`--file`, is refused by the CLI with exit 2 before anything is read.

## Real test

`tests/lists/notes.mjs` runs the built CLI against a dedicated real mailbox
holding at least one stored message. It keeps a note on the newest message
and checks `message show` and `message list` carry it, replaces it and checks
the new text and that `--contains` with a word only the note holds finds
exactly that message and `--noted` lists it among messages that all carry a
note, checks a blank note is refused with `MESSAGE_NOTE_INVALID` and
an unknown message with `NOT_FOUND`, adds a line with `--append` and checks
it stands below the kept text, checks blank added text is
`MESSAGE_NOTE_INVALID` and changes nothing,
removes the note and checks `note` is
`null` and `--noted` no longer lists it. A message that already had a note gets it back at the end.

```sh
node tests/lists/notes.mjs --organization <ORG> --mailbox <ID> \
  --binary <PATH> --source-revision <COMMIT>
```

Reports remain under `.build/real-tests/notes/<run>/report.json`.
