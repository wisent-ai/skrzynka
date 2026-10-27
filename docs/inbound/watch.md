# Watching stored mail

`skrzynka message watch [--mailbox <ID or address>] [--after <NUMBER>]` (or
`GET /v1/messages/watch?mailbox=&after=` as server-sent events, or the live
**Inbox** of Skrzynka Desktop) hands on every message Skrzynka stores from
now on, as each import stores it, until it is stopped. Another program reacts
to new mail by reading this instead of asking again and again.

- Every stored message carries `stored_order`, a number taken when the
  message is stored. Numbers become visible in the order they were taken (each
  is taken under a lock held until its import commits), so a watcher that has
  read up to a number never later finds a smaller one.
- Without `--after` the watch starts with what is stored when it starts. With
  `--after <NUMBER>` it first hands on every message stored after that number,
  then goes on: a watcher that keeps the last `stored_order` it got and starts
  again with it misses nothing and repeats nothing, whichever process stored
  the mail meanwhile.
- `--mailbox` keeps to one mailbox, named by ID or address (an address that
  names two mailboxes is refused with `MAILBOX_SELECTOR_AMBIGUOUS`).
- The watch wakes on the fleet database's announcement that an import stored
  messages (`skrzynka_received`); it never polls. A message removed between
  being stored and being read is passed over.
- Mail arrives in Skrzynka only through imports: a running `skrzynka serve`
  imports on its own, and `skrzynka sync` imports once.

## CLI

Each message is one line of JSON, `{"stored_order": <NUMBER>, "message":
{…}}`, where `message` is what `skrzynka message show` prints; `--text`
prints the same as `path: value` lines instead. The command runs until it is
stopped and exits non-zero with the error when the watch fails, such as when
the fleet database stops announcing (`RECEIVED_ANNOUNCEMENTS_UNAVAILABLE`).

```sh
skrzynka --organization "$ORG" message watch --mailbox sales@example.com
skrzynka --organization "$ORG" message watch --after "$LAST_STORED_ORDER"
```

## HTTP

`GET /v1/messages/watch` answers `text/event-stream`. Each message is one
event: `event: message`, its `stored_order` as the event's `id`, and
`{"stored_order": <NUMBER>, "message": {…}}` as its `data`.

A client that reconnects sending `Last-Event-ID` with the last id it got (as
browsers' `EventSource` do by themselves) resumes after that message; `after=`
in the query takes precedence over the header. A header that is not a whole
number is refused with `400 WATCH_RESUME_INVALID` before the stream opens; an
unknown mailbox is `404 NOT_FOUND`. A watch that fails once open ends with one
`event: refused` whose `data` is `{"code", "message", "retryable"}` (such as
`RECEIVED_ANNOUNCEMENTS_UNAVAILABLE`), and the stream closes; reconnecting
with the last event's id resumes it.
