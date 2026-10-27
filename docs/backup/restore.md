# Restoring an export

`skrzynka message restore --mailbox <ID> --input <DIRECTORY> --folder
<FOLDER>` (Skrzynka Desktop: a folder name and **Restore an export…** in the
mailbox inspector's Export section) puts the originals a
[`message export`](export.md) wrote back into one folder of the mailbox's
provider:

- `manifest.json` in the directory is read; each file it lists under
  `exported` is read and its SHA-256 compared with the one the export
  recorded, so a file changed since is refused (`RESTORE_FILE_CHANGED`) and
  never sent to the provider;
- the folder is selected over IMAP and searched for the file's Message-ID;
  a message the folder already holds is not appended again (`appended:
  false`), so restoring the same export twice leaves one copy, and restoring
  into a folder that still holds the mail changes nothing;
- otherwise the file is appended whole with IMAP `APPEND` (RFC 3501): the
  provider holds it again exactly as it was exported. A message that states
  no Message-ID cannot be looked for and is appended.

The command prints `{mailbox_id, folder, restored, refused}`: every file put
back or found already there, with its Message-ID and `appended`, and every
file refused with its code and sentence. A refused file does not stop the
others; when any was refused the command ends with `RESTORE_INCOMPLETE`
(exit 1) after printing.

Restoring into INBOX makes the next sync import the messages that came
back; Skrzynka imports only what lies past the mailbox's cursor, so appended
messages get new UIDs and are imported as new messages, beside any stored
copy kept from before.

Each file is one request to the core: `POST /v1/mailboxes/:id/append` with
`{folder, content_base64}` answers `{folder, message_id, appended}` (role
`member`), which Desktop calls for each file and which any other client can
use to put one message into a folder.

## Refusals

| Code | When |
| --- | --- |
| `NOT_FOUND` | The organization holds no mailbox with that ID. |
| `RESTORE_INPUT_INVALID` | `manifest.json` is missing, unreadable or not an export manifest. |
| `RESTORE_FILE_INVALID` | A listed file could not be read, or the manifest names a path outside the directory. |
| `RESTORE_FILE_CHANGED` | A file's SHA-256 differs from the one the export recorded. |
| `MESSAGE_APPEND_INVALID` | The folder is blank, the file is empty or does not read as RFC 5322 headers, or (HTTP) `content_base64` is not base64. |
| `IMAP_APPEND_FAILED` | The provider refused `SELECT`, the Message-ID search or `APPEND`, with its own answer, such as a folder that does not exist. |
| `RESTORE_INCOMPLETE` | At least one file was refused; `refused` names each and why. |

Credential and connection refusals are those of a sync.

## Real test

`tests/backup/restore.mjs` runs the built CLI against a dedicated real
mailbox and a provider folder the operator dedicates to the test. It exports
the mailbox into the run's report folder, restores the export into that
folder and checks every exported file is accounted for as restored or
refused with a code, restores it again and checks nothing is appended the
second time, and checks a copy of the export with one file changed is
refused with `RESTORE_FILE_CHANGED` for that file. The appended messages
stay in the dedicated folder.

```sh
node tests/backup/restore.mjs --organization <ORG> --mailbox <ID> \
  --folder <FOLDER> --binary <PATH> --source-revision <COMMIT>
```

Reports remain under `.build/real-tests/restore/<run>/report.json`.
