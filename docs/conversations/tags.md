# Tags

Tags are your own labels on stored messages. They live only in Skrzynka's
database and are never written to the provider, so they cannot move or flag
anything there.

| Surface | Change | Read |
| --- | --- | --- |
| CLI | `skrzynka message tag <ID> --add <TAG> --remove <TAG>` (each repeatable); `skrzynka message tags --rename <TAG> --to <NEW>`, `--remove <TAG>` | `skrzynka message tags [--mailbox <ID>] [--contains <TEXT>] [--since <WHEN>] [--until <WHEN>]`; `skrzynka message list --tag <TAG>`, `--untagged` |
| HTTP | `POST /v1/messages/:id/tags` with `{add, remove}`; `POST /v1/tags/:tag/rename` with `{to}`; `DELETE /v1/tags/:tag` | `GET /v1/tags[?mailbox_id=&contains=&since=&until=]`; `GET /v1/messages?tag=<TAG>`, `?untagged=true` |
| Desktop | **Tags** in the message inspector: **Add**, **Remove**; **Tags** while no message is selected: **Show mail**, **Rename**, **Remove from every message** | **Tags** with **Counted in**, **Counted on mail of** (any time, the last week, the last 30 days, or before the last 30 days: `since=-7d\|-30d`, `until=-30d`) and **Tag contains** / **Find**; the filter bar's **Tag** field and **Untagged only** |

Every message read carries `tags`, trimmed, in lower case and in tag order.
A change adds and removes in one transaction and answers the message.
Adding a tag the message carries, or removing one it does not, is no change.
`message tags` answers `{tag, messages}` for every tag in use, most used
first; `--mailbox <ID or address>` counts only that mailbox's messages (one
the organization does not hold is `NOT_FOUND`; over HTTP an unknown
`mailbox_id` counts nothing and lists `[]`), and `--contains <TEXT>` lists
only the tags holding the text, case aside (a blank one is
`MESSAGE_FILTER_INVALID`). `--since <WHEN>` and `--until <WHEN>` count only
the messages received at or after and before an RFC 3339 instant, a
`YYYY-MM-DD` day or a span back such as `-30d`, so a tag none of them
carries is not listed: the tags used lately (Desktop's **Counted on mail
of**: the last week or 30 days). Blocked senders', snoozed and muted mail
is counted as without a window; an unreadable instant or a window whose
start is not before its end is `MESSAGE_FILTER_INVALID`. Deleting a mailbox's local mail deletes its messages' tags with them.
`message tags … --csv <PATH>` (HTTP `GET /v1/tag-counts/csv` with the same
parameters, `text/csv` offered as `tags.csv`, not under `/v1/tags/` where a
path names one tag; Desktop: **Export as a table (.csv)…** in **Tags**)
writes the tags listed to a new file as a CSV table under the header `tag,
messages` and prints `{path, tags, size_bytes}`; an existing path is
`TAG_OUTPUT_EXISTS` before anything is read, one that cannot be checked,
created or written `TAG_OUTPUT_INVALID`.
`message list --untagged` keeps the messages carrying no tag, the mail not
sorted yet; a saved view keeps it. Given with `--tag` (or `untagged=true`
beside `tag=`) it is refused: the CLI refuses the pair before dispatch, and
HTTP with `MESSAGE_FILTER_INVALID`.

`skrzynka message tag --view <NAME> --add <TAG> --remove <TAG>` (HTTP `POST
/v1/views/:name/tags`; Desktop: a tag and **Tag view** in the filter bar)
changes every message a [saved view](../messages/filters.md) lists, and
answers `{view, messages}` with each message changed. The change is checked
once before any message is touched; an unknown view is `VIEW_NOT_FOUND`.
`message tag --thread <MESSAGE_ID> …` (HTTP `POST
/v1/messages/:id/thread/tags`; Desktop: a tag with **Tag conversation** or
**Untag conversation** in the message inspector's Conversation section)
does the same for every message of the conversation the message belongs
to, as [`message thread`](thread.md) lists it, and answers `{thread,
messages}`; later mail of the conversation is not tagged by it (a [tag
rule](../lists/rules.md) tags arriving mail). Exactly one of the message ID,
`--view` and `--thread` is named.

## Renaming and removing a tag

`message tags --rename <TAG> --to <NEW>` renames a tag wherever the
organization uses it, in one transaction: every message carrying it carries
`<NEW>` instead (one carrying both keeps one), every [tag rule](../lists/rules.md)
tagging with it tags with `<NEW>`, and every saved view keeping it keeps
`<NEW>`. It answers `{tag, to, messages, rules, views}`, the counts of what
changed. `message tags --remove <TAG>` takes a tag off every message and
answers `{tag, messages}`; rules and views naming it are kept, so a rule
tags new mail with it again. A tag nothing uses is `MESSAGE_TAG_NOT_FOUND`.

## Refusals

| Code | Message |
| --- | --- |
| `MESSAGE_TAG_INVALID` | `name a tag to add or to remove` |
| `MESSAGE_TAG_INVALID` | `tag "<TAG>" is not a tag: write a non-empty name without commas or control characters` |
| `MESSAGE_TAG_INVALID` | `tag "<TAG>" is both added and removed; name it once` |
| `MESSAGE_TAG_INVALID` | `"<TAG>" is already the tag's name; name another` |
| `MESSAGE_TAG_NOT_FOUND` (404) | `no message, rule or view of the organization uses the tag "<TAG>"; …` |
| `MESSAGE_FILTER_INVALID` | `the tag filter is empty; name what it must match or leave the filter out` |
| `NOT_FOUND` | `message was not found` |
| `VIEW_NOT_FOUND` | `there is no view "<NAME>"; there are: …` |
| `TAG_OUTPUT_EXISTS`, `TAG_OUTPUT_INVALID` | `tags --csv` names a path that exists, or one that cannot be checked, created or written |

## Real test

`tests/conversations/tags.mjs` runs the built CLI against a dedicated real
mailbox. On its newest stored message it adds a freshly drawn tag, checks
`message tags --mailbox` with its mailbox's ID and with its address counts
it the same (and an unknown mailbox is `NOT_FOUND`), that `--contains` the tag in upper case lists it alone and a
blank one is refused, that `--csv` with `--contains` the tag writes exactly
the rows the listing holds, tag and count, and refuses the same path again
with `TAG_OUTPUT_EXISTS`, that `--since` the message's receive instant counts
it and `--until` that instant does not, and that a backwards window is
refused, checks the
message, `message list --tag` and `message tags` all show it, checks a
repeated add changes nothing, checks `message list --untagged` keeps exactly
the mailbox's messages carrying no tag and not the one just tagged, removes it and checks it is gone everywhere,
and checks the refusals for an empty tag and a tag both added and removed.
It tags the newest message's conversation with `--thread` and checks exactly
the messages `message thread` lists carry the tag, then removes it the same
way and checks an ID beside `--thread` is refused.
It then saves a view of the newest message's sender, tags it with `--view`
and checks exactly the view's messages carry the tag, saves a view keeping
the tag, renames the tag and checks the messages and that view carry the new
name and the old one lists nothing, removes the new name from every message,
checks a second rename and remove are `MESSAGE_TAG_NOT_FOUND`, and checks an
unknown view is `VIEW_NOT_FOUND`. It leaves every message with the tags it
had and deletes the views.

```sh
node tests/conversations/tags.mjs --organization <ORG> --mailbox <ID> \
  --binary <PATH> --source-revision <COMMIT>
```

Reports remain under `.build/real-tests/message-tags/<run>/report.json`.
