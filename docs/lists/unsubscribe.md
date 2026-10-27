# Leaving mailing lists

Each import records the mailing-list headers a message states: `List-Id`
(RFC 2919), `List-Unsubscribe` (RFC 2369) and `List-Unsubscribe-Post`
(RFC 8058). Skrzynka groups the stored mail by those lists and asks a list to
stop sending by the way out its own message names.

| Surface | Lists | Leave one |
|---|---|---|
| CLI | `skrzynka message subscriptions [--mailbox ID] [--subscribed] [--contains TEXT] [--since WHEN] [--until WHEN] [--leavable]` | `skrzynka message unsubscribe (MESSAGE_ID \| --list LIST_ID [--mailbox ID]) --idempotency-key KEY` |
| API | `GET /v1/subscriptions[?mailbox_id=ID&subscribed=true&contains=TEXT&since=WHEN&until=WHEN&leavable=true]` | `POST /v1/messages/:id/unsubscribe` with `{"idempotency_key": KEY}`; `POST /v1/subscriptions/unsubscribe` with `{list_id, mailbox, idempotency_key}` |
| Desktop | Inbox, the inspector while no message is selected: **Mailing lists**, with **Mailbox**, **Still subscribed only**, **Can leave from here only**, **Last message** and **List-Id or sender contains** with **Find** | **Unsubscribe from …** beside a list |

## The lists

`message subscriptions` answers `lists` and `messages_not_read`. A list is the
stored messages of one mailbox sharing a `List-Id` (compared without case),
or, for a list that states none but names a way out, one sender. Each list
carries `list_id`, `sender`, `mailbox_id`, `messages`, the newest message
(`latest_message_id`, `latest_received_at`), the `ways_out` that message names
(`one_click` with `url`, `mailto` with `address`, `subject`, `body`, or `page`
with `url`), `ways_out_unreadable` when its header cannot be read, and
`last_attempt`, the newest request to leave it. Lists with the most messages
come first. `messages_not_read` counts messages imported before list headers
were recorded; their `mailing_list` is `null` until one of them is listed for
attachments or unsubscribed, which reads it from the provider once.
`--subscribed` keeps only the lists no request to leave has been sent to:
those whose `last_attempt` is absent or did not end `sent`, so what is left
to tidy up is listed alone. `--contains <TEXT>` keeps the lists whose
`list_id` or `sender` contains the text, case aside, in the same order, to
find one list among many; a blank text is `MESSAGE_FILTER_INVALID`.
`--since <WHEN>` keeps the lists whose newest stored message arrived at or
after an RFC 3339 instant, a `YYYY-MM-DD` day read as its UTC start or a
span back from now such as `-30d`, the lists still writing, and `--until`
those whose newest message arrived before one, the lists gone quiet (Desktop's
**Last message**: **Within 30 days** or **Quiet for 90 days**). A window
whose start is not before its end is `MESSAGE_FILTER_INVALID`, refused
before any mail is read. `--leavable` keeps the lists `message unsubscribe`
can leave itself: those whose `ways_out` name a `one_click` or `mailto`
way, not only `page`s, which Skrzynka never opens (Desktop's **Can leave
from here only**).

`message subscriptions … --csv <PATH>` (HTTP `GET /v1/subscriptions/csv`
with the same parameters, `text/csv` offered as `subscriptions.csv`;
Desktop: **Export as a table (.csv)…** in **Mailing lists**) writes the
lists kept, in the listing's order, to a new file as a CSV table (RFC 4180,
CRLF lines) under the header `list_id, sender, mailbox_id, messages,
latest_message_id, latest_received_at, ways_out, last_attempt_status,
last_attempt_at`, `ways_out` naming each way by its `method` and target
(`one_click <URL>`, `mailto <ADDRESS>`, `page <URL>`, `; ` between them) and
the last request's status and claim instant empty when none was sent, and
prints `{path, lists, size_bytes}`. An existing path is
`SUBSCRIPTION_OUTPUT_EXISTS` before anything is read; a path that cannot be
checked, created or written is `SUBSCRIPTION_OUTPUT_INVALID`.

## Leaving one

`message unsubscribe` uses the message's own ways out:

1. a one-click address (an HTTPS URL with `List-Unsubscribe-Post:
   List-Unsubscribe=One-Click`): Skrzynka POSTs `List-Unsubscribe=One-Click`
   and any 2xx answer is acceptance;
2. otherwise a `mailto:` address: Skrzynka mails it from the mailbox that
   received the message, with the subject and body the address names and no
   others;
3. a list that names only pages is refused: a page needs a person in a
   browser, and Skrzynka opens none.

The request is claimed by its idempotency key before the list is asked; the
same key returns the first attempt. The attempt walks `pending → sending →
sent | failed | uncertain` and carries `method`, `target`, the unsubscribe
mail's `provider_message_id`, and the refusal. Nothing at the provider other
than that one request changes.

`message unsubscribe --list <LIST_ID> [--mailbox <ID or address>]
--idempotency-key <KEY>` (HTTP `POST /v1/subscriptions/unsubscribe` with
`{list_id, mailbox, idempotency_key}`, `mailbox` optional) names the list by
the List-Id `message subscriptions` prints, case aside, and asks it through
its newest stored message exactly as `message unsubscribe <ID>` of that
message would, the same key answering the same attempt. Skrzynka Desktop's
**Unsubscribe** beside a list that states a List-Id takes this route with
the list's mailbox. A List-Id several mailboxes receive needs `--mailbox`.

## Leaving many

`message subscriptions <the listing's flags> --leave --idempotency-key <KEY>`
(HTTP `POST /v1/subscriptions/leave` with the listing's parameters and
`{"idempotency_key": KEY}`, role `member`; Desktop: **Leave every list
listed…** in **Mailing lists**, after a confirmation) asks every list the
listing keeps to stop sending, each through its newest stored message
exactly as `message unsubscribe <ID>` of that message would, claimed by
`<KEY>-<that message's id>`: the same command again asks no list twice while
no newer message of it arrived. Such as `--subscribed --leavable --until
-90d`, every list gone quiet for three months that can be left from here.
It prints `{left, refused}`: each list's attempt, and each list that could
not be asked (`{list_id, sender, mailbox_id, code, message}`, such as one
naming only pages, `UNSUBSCRIBE_NEEDS_BROWSER`), the others still asked.
The listing must be narrowed by `--contains`, `--since` or `--until`, so
nothing leaves every list at once: otherwise `UNSUBSCRIBE_LIST_INVALID`;
one keeping no list is `UNSUBSCRIBE_LIST_NOT_FOUND`. Without
`--idempotency-key`, and with `--csv`, it is refused by Clap.

## Refusals

| Code | Sentence | What to do |
|---|---|---|
| `IDEMPOTENCY_KEY_REQUIRED` | `message unsubscribe needs --idempotency-key: the same key on a repeated command returns the first attempt instead of sending again` | Name a key. |
| `IDEMPOTENCY_KEY_INVALID` | `idempotency_key must contain non-whitespace characters only` | Correct the key. |
| `IDEMPOTENCY_KEY_REUSED` | `idempotency key already belongs to a different unsubscribe request` | Use a new key for a new request. |
| `UNSUBSCRIBE_LIST_INVALID` | `the List-Id is blank; name it as message subscriptions prints it`, or `--leave` narrowed by no text or window | Name the List-Id, or narrow the lists to leave. |
| `UNSUBSCRIBE_LIST_NOT_FOUND` (404) | `no stored message names the List-Id "{id}"; message subscriptions lists the lists there are`, or `--leave` keeping no list | Take the List-Id from `message subscriptions`, or sync first. |
| `UNSUBSCRIBE_LIST_AMBIGUOUS` (409) | `the List-Id "{id}" reaches mailboxes {ids}; name one with --mailbox` | Name the mailbox. |
| `NOT_FOUND` | `message was not found` | Use an ID `message list` prints. |
| `UNSUBSCRIBE_UNAVAILABLE` | `message {id} names no way to unsubscribe: the message states no List-Unsubscribe header` (or why its header cannot be read) | The sender gives no way out; leave the list at the sender's site. |
| `UNSUBSCRIBE_NEEDS_BROWSER` | `message {id} names only pages a person opens ({urls}); Skrzynka opens no browser, so open one of them yourself` | Open the page. |
| `UNSUBSCRIBE_UNREACHABLE` | `one-click unsubscribe POST to {url} got no answer: {cause}` | Retry with a new key; asking twice is harmless. |
| `UNSUBSCRIBE_REFUSED` | `one-click unsubscribe POST to {url} was answered HTTP {status}` | The list refused; use another way out. |
| `UNSUBSCRIBE_ADDRESS_INVALID` | `the list's unsubscribe address {address} is not a valid address: {cause}` | The list's header is broken. |
| `UNSUBSCRIBE_UNCERTAIN` | the SMTP server's answer was lost, or `the unsubscribe request was interrupted before the list answered` | Check the mailbox's Sent mail before asking again. |
| `SUBSCRIPTION_OUTPUT_EXISTS`, `SUBSCRIPTION_OUTPUT_INVALID` | `<PATH> already exists; …`, or the path could not be checked, created or written | Name a new path. |

SMTP and credential refusals (`SMTP_REJECTED`, `SMTP_TLS_FAILED`,
`MAILBOX_PROFILE_INVALID`, Skarbiec codes) are those of replies. Reading an
older message's headers can refuse with `MESSAGE_SOURCE_GONE` or
`MESSAGE_SOURCE_CHANGED`, as for attachments.

## Proof

`tests/lists/unsubscribe.mjs` runs the real CLI against a dedicated mailbox
whose synced mail includes a message from a test list the operator owns and a
message from no list. It reads the lists, unsubscribes once, repeats the same
key and checks the same attempt comes back, checks `--list` with the test
list's List-Id in upper case and the mailbox answers that same attempt
(the test message must be its list's newest), that a List-Id no mail names
is `UNSUBSCRIBE_LIST_NOT_FOUND` and a blank one `UNSUBSCRIBE_LIST_INVALID`,
checks `--subscribed` keeps
`--contains` with the test list's List-Id (or sender) in upper case keeps
it and a text no list holds keeps none, checks `--since` and `--until` at
the test list's newest message split the lists there between them and a
backwards window is refused, checks `--leavable` keeps exactly the lists
naming a way out other than a page, checks `--csv` writes one row per
listed list in order with its message count, the test list's row naming
its request's status, and refuses the same path again with
`SUBSCRIPTION_OUTPUT_EXISTS`, leaving the file as it was, and checks the
refusals for a missing key, a blank `--contains` and for the message that
names no way out; then checks `--leave` narrowed by no text or window is
`UNSUBSCRIBE_LIST_INVALID`, narrowed to a text no list holds
`UNSUBSCRIBE_LIST_NOT_FOUND`, and without a key refused, no list's last
request changing.

```bash
node tests/lists/unsubscribe.mjs --organization ORGANIZATION --mailbox MAILBOX_ID \
  --list-message MESSAGE_ID --plain-message MESSAGE_ID \
  --binary target/debug/skrzynka --source-revision HEAD
```
