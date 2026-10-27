# A message's original

Skrzynka stores a message's headers, plain text and the description of its
attachments, not its MIME source. When the whole original is needed (to keep
it as evidence, open it in another mail program, or forward it as an
attachment), Skrzynka reads it from the provider:

| Surface | Form |
| --- | --- |
| CLI | `skrzynka message source <ID> --output <PATH>` |
| HTTP | `GET /v1/messages/:id/source`, typed `message/rfc822`, offered as `<id>.eml` |
| Desktop | **Save original (.eml)…** in the message inspector's Attachments section |

The message is read from its mailbox's INBOX by UID with `BODY.PEEK[]`, so
nothing is marked read there, and only while that UID still holds the message
Skrzynka stored. The CLI writes a new file, never replacing one, removes a
partial file it could not finish, and prints
`{message_id, size_bytes, sha256, path}`. Skrzynka keeps no copy.

## Refusals

| Code | When |
| --- | --- |
| `MESSAGE_SOURCE_OUTPUT_EXISTS` | `--output` names an existing path; checked before the provider is read. |
| `MESSAGE_SOURCE_OUTPUT_INVALID` | The path could not be checked, created or written; the operating system's cause is named. |
| `NOT_FOUND` | The organization holds no message with that ID. |
| `MESSAGE_SOURCE_GONE` | The INBOX no longer holds the UID: the provider deleted, archived or moved it. |
| `MESSAGE_SOURCE_CHANGED` | The UID now holds a different message: the provider renumbered its mail. |

Credential and IMAP refusals are those of attachments
(`docs/messages/attachments.md`).

## Real test

`tests/conversations/source.mjs` runs the built CLI against a dedicated real
mailbox. It saves the newest stored message's original, checks the file's
size and SHA-256 match what the command printed, that the file's `Message-ID`
header is the stored one, that saving to the same path again is refused with
`MESSAGE_SOURCE_OUTPUT_EXISTS` and leaves the file unchanged, and that an
unknown message is refused with `NOT_FOUND`.

```sh
node tests/conversations/source.mjs --organization <ORG> --mailbox <ID> \
  --binary <PATH> --source-revision <COMMIT>
```

Reports and the saved file remain under
`.build/real-tests/message-source/<run>/`.
