# Finding the mail one address received

Skrzynka keeps every message its mailboxes received. To read the ones sent to
one address, such as a sign-up confirmation a provider sent to a generated
address, narrow the listing by recipient and sender. The same two filters
exist on every surface:

| Surface | Form |
|---|---|
| CLI | `skrzynka message list --recipient ADDRESS --sender TEXT` |
| API | `GET /v1/messages?recipient=ADDRESS&sender=TEXT` |
| Desktop | Inbox, the recipient and sender fields above the table, then Apply |

Each also takes `--mailbox` (`mailbox_id`), `--limit` and `--offset`.

## What matches

- **Recipient** matches an address in the To header whole, without case. An
  entry written as a display name with the address in angle brackets matches
  its bracketed address. A longer address that merely ends in the same text is
  a different recipient and does not match.
- **Sender** matches any part of the From header, without case, so a
  provider's sending domain is enough.
- With either filter, `--limit` and `--offset` page the matching messages, not
  the whole inbox: the second match is `--limit 1 --offset 1`.
- No match is an empty list, `[]`, not an error.

## Refusals

| Code | Cause | What to do |
|---|---|---|
| `MESSAGE_FILTER_INVALID` | `the recipient filter is empty; name an address or leave the filter out` (or `sender`) | Name a value, or drop the flag. A blank filter is never read as no filter. |
| `LIMIT_INVALID` | `--limit 0` | Omit `--limit` to list every match. |

## Example

A purchase journey signed an account up with a generated address and waits for
the provider's code:

```bash
skrzynka sync
skrzynka message list --recipient ADDRESS --sender anthropic.com --limit 1
```

The newest matching message is first; its `body_text` holds the code.

## Proof

`tests/messages/filter.mjs` runs the real CLI against a dedicated mailbox that
already holds synced mail. It checks that the recipient filter returns exactly
the messages addressed to the newest message's recipient, in order and without
case; that a longer address does not match; that the sender filter keeps the
message; that a page is cut after the filter; and that a blank recipient or
sender is refused with `MESSAGE_FILTER_INVALID`. It writes its commands, exit
statuses and observations to `.build/real-tests/message-filter/run-*/report.json`.

```bash
node tests/messages/filter.mjs --organization ORGANIZATION --mailbox MAILBOX_ID \
  --binary target/debug/skrzynka --source-revision HEAD
```
