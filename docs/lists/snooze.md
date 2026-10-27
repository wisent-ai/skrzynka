# Snoozing a message

`skrzynka message snooze <ID> --until <INSTANT>` (HTTP:
`POST /v1/messages/:id/snooze` with `{until}`; Skrzynka Desktop: **Snooze**
in the message inspector) puts a stored message out of sight until an
instant:

- `message list` and `GET /v1/messages` leave it out until the instant
  passes, then list it again; nothing has to run for it to come back;
- `--include-snoozed` (`include_snoozed=true`; Desktop: **Include snoozed**)
  lists it anyway, and `--snoozed` (`snoozed=true`; Desktop: **Snoozed
  only**) lists the snoozed messages alone, each with when it wakes;
- `message show` and every other read still answer it, with
  `snoozed_until` naming the instant (UTC); a message never snoozed has
  `snoozed_until: null`;
- nothing changes at the provider: the message stays where it is, read or
  unread as it was.

`--until` is an RFC 3339 instant ahead of now, or a span from now: `+`, a
whole number and a unit, `m` minutes, `h` hours, `d` days or `w` weeks
(`+2h`, `+3d`, `+1w`), the same in `{until}` over HTTP. Desktop's **Snooze
for** menu sends `+1h`, `+1d` or `+1w`.
A span is counted from when the request is read. Snoozing a snoozed message
moves its instant. `skrzynka message wake <ID>` (`DELETE
/v1/messages/:id/snooze`; Desktop: **Wake now**) removes the snooze now;
waking a message that is not snoozed is no change.

`skrzynka message snooze --view <NAME> --until <INSTANT>` (`POST
/v1/views/:name/snooze`; Desktop: an instant and **Snooze view** in the
filter bar) snoozes every message a [saved view](../messages/filters.md)
lists, and answers `{view, until, messages}`. The instant is checked once
before any message is touched; an unknown view is `VIEW_NOT_FOUND`.
`skrzynka message wake --view <NAME>` (`DELETE /v1/views/:name/snooze`;
Desktop: **Wake view** in the filter bar) shows every message the view keeps
that is snoozed now again, though the view's own listing leaves snoozed mail
out, and answers `{view, messages}` with the messages woken.

`skrzynka message snooze --thread <MESSAGE_ID> --until <INSTANT>` and
`message wake --thread <MESSAGE_ID>` (`POST` / `DELETE
/v1/messages/:id/thread/snooze`; Desktop: **Snooze conversation for** and
**Wake conversation** in the message inspector's Conversation section) do
the same for every message of the conversation the message belongs to, as
[`message thread`](../conversations/thread.md) lists it, and answer
`{thread, until, messages}` (`until` `null` for a wake). A reply arriving
later is not snoozed by it; to keep a conversation out of sight for good,
[mute](../conversations/thread.md) it. Exactly one of the message ID,
`--view` and `--thread` is named.

A snooze lives in Skrzynka's database with the message, so every process
on that database (the CLI, a running service, Desktop) sees the same one,
and deleting the message's mailbox removes it.

## Refusals

| Code | When |
| --- | --- |
| `SNOOZE_INVALID` | `--until` is neither an RFC 3339 instant nor a span such as `+2h`, has already passed (a span of zero or less included), or reaches past the last instant Skrzynka can store. |
| `NOT_FOUND` | The organization holds no message with that ID. |
| `VIEW_NOT_FOUND` | `--view` names no saved view; the refusal lists the ones there are. |

## Real test

`tests/lists/snooze.mjs` runs the built CLI against a dedicated real
mailbox holding at least one stored message. It snoozes the newest message
for `--lead-seconds`, checks `message list` leaves it out and
`--include-snoozed` lists it, checks `--snoozed` lists it and only messages
snoozed past now, checks an instant that far in the past is
refused with `SNOOZE_INVALID`, as are a span with an unknown unit (`+2y`) and
a span of zero, snoozes it again for `+1d` and checks `snoozed_until` lies a
day ahead, wakes it and checks it is listed again with
`snoozed_until: null`. It then saves a view of just that message (a freshly
drawn tag), snoozes it with `--view` and checks the answer names exactly that
message and the listing leaves it out, wakes it with `wake --view` and checks
the answer names exactly that message, the listing shows it again and a
second `wake --view` wakes none, then removes the tag and the
view. It then snoozes the message's whole conversation with `--thread` and
checks the answer names exactly the messages `message thread` lists and the
listing leaves each out, and wakes them with `wake --thread`. A failed run
wakes the conversation and removes the tag and view before it ends.

```sh
node tests/lists/snooze.mjs --organization <ORG> --mailbox <ID> \
  --lead-seconds <SECONDS> --binary <PATH> --source-revision <COMMIT>
```

Reports remain under `.build/real-tests/snooze/<run>/report.json`.
