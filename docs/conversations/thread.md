# Conversations

`skrzynka message thread <ID>` (HTTP `GET /v1/messages/:id/thread`, Skrzynka
Desktop: **Conversation** in the message inspector) answers every stored
message of the same mailbox that belongs to the conversation of message `ID`,
oldest first by when each was received, the message itself included.

## What ties messages together

The headers decide, as the senders' mail programs wrote them:

- every message carries its own `Message-ID`;
- a reply names the message it answers in `In-Reply-To`;
- a reply's `References` lists the Message-IDs of the conversation so far.

The conversation starts with every Message-ID the given message carries or
names. A stored message joins when any Message-ID it carries or names is
already among them, and brings its own; this repeats until no message joins.
Two messages that both answer a third the mailbox no longer holds still meet
through the Message-ID they both name.

Nothing is guessed from subjects: two messages titled the same with no header
tying them are two conversations. A message whose headers name no other
stored message is a conversation of one. Mail Skrzynka sent (replies,
originated messages) is not stored as received mail and is not listed; read
it with `skrzynka message replies` and `skrzynka message outbound`.

The read uses only what Skrzynka stored and contacts no provider.

## Muting a conversation

`skrzynka message mute <ID>` (HTTP `POST /v1/messages/:id/mute`; Desktop:
**Mute conversation** in the Conversation section) keeps every Message-ID the
conversation of message `ID` carries or names as muted for its mailbox.
Listings (`message list`, `GET /v1/messages`, saved views, the inbox) then
leave out every message of the mailbox that carries or names one of them —
the conversation as it stands and each later reply to it as it arrives, since
a reply names the conversation in its headers. `--include-muted`
(`include_muted=true`, Desktop's **Include muted**) lists them anyway, and
`--muted` (`muted=true`, Desktop's **Muted only**; a saved view keeps it)
lists only the mail of muted conversations.
`message unmute <ID>` (`DELETE /v1/messages/:id/mute`; Desktop **Unmute**)
stops keeping every Message-ID the conversation carries or names, and lists
it again. Both answer `{message_id, mailbox_id, messages, identities,
changed}`: the conversation's stored messages, the Message-IDs it carries or
names, and how many were kept or stopped being kept now. Muting a
conversation already muted keeps nothing new, so its `changed` is none.

`message mute --view <NAME>` and `message unmute --view <NAME>` (`POST` /
`DELETE /v1/views/:name/mute`; Desktop: **Mute view** and **Unmute view** in
the filter bar's views) do the same for the conversation of every message
a [saved view](../messages/filters.md) keeps, muted mail included, each
conversation once, and answer `{view, conversations, refused}`: every
change as one mute answers it, and every conversation refused (one with no
Message-ID to keep) with its `message_id`, `code` and `message`. A
conversation already as asked is passed over, so a second mute of the same
view changes none. Exactly one of the message ID and `--view` is named; an
unknown view is `VIEW_NOT_FOUND`.

Nothing changes at the provider and nothing is deleted. Exports and
`mailbox summary` include muted mail; the summary counts it as `muted`.
Removing a mailbox's local mail removes its muted Message-IDs.

## Refusals

| Code | When |
| --- | --- |
| `NOT_FOUND` | The organization holds no message with that ID. |
| `CONVERSATION_UNMUTABLE` | `mute`: the conversation's messages carry no Message-ID, In-Reply-To or References header, so later mail of it could not be recognized. |
| `CONVERSATION_NOT_MUTED` | `unmute`: no Message-ID of the conversation is muted. |
| `VIEW_NOT_FOUND` | `--view` names no saved view; the refusal lists the ones there are. |

## Real test

`tests/conversations/thread.mjs` runs the built CLI against a dedicated real
mailbox that already holds a reply to another stored message. It finds such a
pair from the stored headers, confirms the conversation of either message
holds both, is ordered by receipt and holds only messages tied by headers,
and that an unknown message ID is refused with `NOT_FOUND`. It then mutes the
conversation from the reply and checks the listing leaves every member out,
`--include-muted` lists them, `--muted` lists them and nothing that is
listed unmuted, muting it again from the other end keeps
nothing new, unmuting lists them again, and a second unmute is refused with
`CONVERSATION_NOT_MUTED`. A saved view of the reply (a freshly drawn tag)
is then muted with `--view`, which must change exactly that one
conversation and leave it out of the listing, a second `mute --view`
changes none, and `unmute --view` must change it back and list it again;
the view and tag are removed after, as a failed run removes them. It sends nothing.

```sh
node tests/conversations/thread.mjs --organization <ORG> --mailbox <ID> \
  --binary <PATH> --source-revision <COMMIT>
```

Reports remain under `.build/real-tests/message-thread/<run>/report.json`.
