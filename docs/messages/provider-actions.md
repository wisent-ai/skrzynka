# Changing a message at its provider

Skrzynka imports INBOX messages and keeps its own copy of each. Three actions
change the mailbox's copy at the provider instead: marking it seen, unseen,
flagged or unflagged (Gmail shows a flagged message as starred),
moving it out of INBOX into another folder, and deleting it. Skrzynka's stored
message is never changed or removed by them.

| Action | CLI | API | Desktop |
|---|---|---|---|
| Mark seen, unseen, flagged or unflagged | `skrzynka message mark ID --as seen\|unseen\|flagged\|unflagged` | `POST /v1/messages/:id/mark` `{"as": "flagged"}` | Inbox inspector, At the provider: Mark seen, Mark unseen, Mark flagged, Mark unflagged |
| Move to a folder | `skrzynka message move ID --folder NAME`, or `--to archive\|trash\|junk\|all` | `POST /v1/messages/:id/move` `{"folder": "NAME"}` or `{"to": "archive"}` | Move, with the folder named; **Move to** with the use |
| Delete at the provider | `skrzynka message delete ID --confirm` | `POST /v1/messages/:id/delete` | Delete at the provider, after a confirmation |
| Read what was asked | `skrzynka message actions ID` | `GET /v1/messages/:id/actions` | the list under the actions |

The three changes need the `member` role, as sending does.

## What one action does

1. A record is claimed as `pending` for the message, before anything reaches
   the provider.
2. The mailbox's credential is resolved from Skarbiec and an IMAP session
   opens INBOX.
3. The provider's copy is read by the message's UID with `BODY.PEEK[]`, so
   reading it marks nothing seen, and it must be the message Skrzynka stored
   under that UID: the same headers it kept and the same text.
4. The change is made:
   - **mark**: `UID STORE +FLAGS.SILENT (\Seen)` or `-FLAGS.SILENT (\Seen)`,
     and `\Flagged` the same way for flagged and unflagged;
   - **move**: `UID MOVE` into the folder exactly as named (RFC 6851); the
     server must announce `MOVE`;
   - **delete**: `UID STORE +FLAGS.SILENT (\Deleted)` then `UID EXPUNGE` of
     that one UID (RFC 4315); the server must announce `UIDPLUS`, because a
     plain `EXPUNGE` would also remove every message other clients marked
     deleted.
5. The provider's state is read again: the flag must be set or cleared, and a
   moved or deleted message must no longer be in INBOX.
6. The record ends `done`; or `failed` with the refusal; or `uncertain` when
   the connection was lost after the command was sent, so whether the provider
   acted is not known.

A `done` or `uncertain` action prints its record and exits 0; a refused one
prints the error and exits 1, and its record says `failed` with the same code
and sentence. What a provider does with a deleted INBOX message is its own
setting: Gmail, for example, may keep it in All Mail.

## The record

Each action is `{id, message_id, action, folder, status, error_code,
error_message, requested_at, completed_at}`: `action` is `seen`, `unseen`,
`flagged`, `unflagged`, `move` or `delete`; `folder` is the folder a move named and `null` otherwise;
`status` is `pending`, `done`, `failed` or `uncertain`. `message actions` lists
them newest first; a message nothing was asked of has `[]`.

## Which folders there are

`skrzynka mailbox folders <MAILBOX_ID>` (HTTP: `GET
/v1/mailboxes/:id/folders`; Skrzynka Desktop: the **Folders** menu beside
**Move**) reads the provider's folders now with one `LIST "" *` and prints
each as `{name, delimiter, selectable, special_use, attributes}`: `name` is
exactly what `message move --folder` takes, `selectable` is `false` for a
`\Noselect` name that only holds other folders, `special_use` is what the
provider says the folder is for (RFC 6154: `all`, `archive`, `drafts`,
`flagged`, `junk`, `sent`, `trash`) or `null`, and `attributes` keeps every
other attribute as the provider wrote it. Nothing is stored. A refused
`LIST` is `IMAP_LIST_FAILED` with the server's answer; credential and
connection refusals are those of a sync.

## Moving by what a folder is for

`--to archive|trash|junk|all` (HTTP `{"to": …}` in place of `folder`;
Desktop: **Move to** under **Move**, and **Move view to** in the filter bar)
moves to the selectable folder the mailbox's provider declares for that use,
read with `LIST` when the move is asked, so the same command archives mail
in Gmail (`[Gmail]/All Mail`, declared `all`) and in a provider whose archive
folder is called something else. The record's `folder` is the folder found.
Exactly one of `--folder` and `--to` is given. A provider that declares no
such folder is refused with `MESSAGE_FOLDER_UNDECLARED`, naming the folders
it lists; nothing is moved. For a view, the folder is read once per mailbox,
and a mailbox whose provider declares none has each of its messages refused.

## Every message of a view

`skrzynka message mark --view <NAME> --as seen|unseen|flagged|unflagged` and `skrzynka message
move --view <NAME> --folder <FOLDER>|--to <USE>` (HTTP: `POST /v1/views/:name/mark` and
`/move`; Desktop: **Mark view** with the state and a folder with **Move view** in the
filter bar) ask the provider to change every message a [saved
view](filters.md) lists. Each message is its own recorded action, exactly as
if it were named by its ID, and prints `{view, actions, refused}`: the record
of every message changed, and every message refused with its code and
sentence. A refused message does not stop the others; when any was refused
the CLI ends with `VIEW_ACTION_INCOMPLETE` (exit 1) after printing. Deleting
is asked of one message at a time; there is no delete for a view.

## Every message of a conversation

`skrzynka message mark --thread <MESSAGE_ID> --as …` and `skrzynka message
move --thread <MESSAGE_ID> --folder <FOLDER>|--to <USE>` (HTTP: `POST
/v1/messages/:id/thread/mark` and `/thread/move`; Desktop: **At the
provider** in the message inspector's Conversation section, with **Move
all** for a folder) do the same for every message of the conversation the
message belongs to, as [`message thread`](../conversations/thread.md) lists
it: archiving or marking a whole conversation at once. They print `{thread,
actions, refused}` and end with `THREAD_ACTION_INCOMPLETE` (exit 1) when any
message was refused. Exactly one of the message ID, `--view` and `--thread`
is named; Clap refuses none or two with exit 2.

## Refusals

| Code | Cause | What to do |
|---|---|---|
| `NOT_FOUND` | `message was not found` | The ID names no message of this organization; take it from `skrzynka message list`. |
| `CONFIRMATION_REQUIRED` | `message delete ID needs --confirm: …` | Add `--confirm`. |
| `MESSAGE_FOLDER_INVALID` | `the folder to move to is empty; …`, `the message is already in INBOX; …`, or neither or both of folder and to | Name a folder the provider lists, other than INBOX, or a use with `to`. |
| `MESSAGE_FOLDER_UNDECLARED` | `the provider of mailbox … declares no archive folder (RFC 6154); name one with --folder from [...]` | Name the folder with `--folder`. |
| `MESSAGE_SOURCE_GONE` | `INBOX of … no longer holds UID N, where message … was imported from: …` | The provider already deleted, archived or moved it; nothing is left to change. |
| `MESSAGE_SOURCE_CHANGED` | `UID N in INBOX of … now holds a different message than message …; nothing was changed at the provider` | The provider renumbered its mail; sync the mailbox and act on the newly imported message. |
| `IMAP_CAPABILITY_MISSING` | `the IMAP server at … does not announce MOVE (RFC 6851)` or `UIDPLUS (RFC 4315)` … | The provider cannot do this action by UID; do it in the provider's own client. |
| `IMAP_ACTION_REFUSED` | `IMAP <command> for … failed: <the server's answer>` | Read the server's words: a move to a folder that does not exist is refused here. |
| `IMAP_ACTION_UNCONFIRMED` | `UID N in INBOX of …: the provider accepted … yet …` | The provider said yes but its state disagrees; check the mailbox in the provider's client. |
| `IMAP_ACTION_UNCERTAIN` | recorded on an `uncertain` action: `IMAP <command> for … failed: <the connection error>` | Ask again: the provider's copy is read first, so a message already moved or deleted answers `MESSAGE_SOURCE_GONE`. |
| `VIEW_ACTION_INCOMPLETE`, `THREAD_ACTION_INCOMPLETE` | `N of the view's (the conversation's) messages were not changed; each is listed under refused` | Read each refusal; the others were changed. |

Credential and connection refusals before the action (`SKARBIEC_*`,
`IMAP_UNAVAILABLE`, `IMAP_AUTHENTICATION_FAILED`, `IMAP_INBOX_UNAVAILABLE`)
are the same as a sync's and are recorded on the action as `failed`.

## Proof

`tests/messages/actions.mjs` runs the real CLI against a dedicated mailbox and
changes two of its messages that exist to be changed. It checks that a delete
without `--confirm`, a blank folder, INBOX as the folder, an unknown message
and an unknown `--as` value are refused before the provider is reached; that
marking one message unseen, seen, flagged and unflagged ends `done`, and that a saved view of
just that message (a freshly drawn tag) marks it seen with `mark --view` as
one `done` action of that message and nothing refused, and that `mark --thread`
of it marks seen exactly the messages `message thread` lists, each `done` or
refused with its code; that `--folder`
with `--to` is refused before dispatch; that moving it ends `done` — by
`--to` with the use the provider declares for the named folder when it
declares one `--to` takes, otherwise into the named folder — with the
record naming that folder, after which acting on it again is `MESSAGE_SOURCE_GONE`
and its stored copy remains, once `mailbox folders` has listed the named
folder as selectable beside INBOX; that `message actions` holds the five
actions newest first with the refused one `failed`; and that deleting the other
message ends `done`, a second delete is `MESSAGE_SOURCE_GONE`, and its stored
copy remains. It writes its commands, exit statuses and observations to
`.build/real-tests/message-actions/run-*/report.json`.

```bash
node tests/messages/actions.mjs --organization ORGANIZATION \
  --move-message MESSAGE_ID --delete-message OTHER_MESSAGE_ID --folder FOLDER \
  --binary target/debug/skrzynka --source-revision HEAD
```
