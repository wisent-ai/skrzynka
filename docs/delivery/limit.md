# Mailbox send limit

A mailbox can carry a limit on how many messages it sends in one UTC day. The
operator sets it; Skrzynka assumes none. It is the honest way to ramp a new
sending address up slowly: raise the limit as the address earns the
receiving servers' trust. It does not hide what is sent or from whom.

| Surface | Read | Set | Remove |
| --- | --- | --- | --- |
| CLI | `skrzynka mailbox limit <ID>` | `skrzynka mailbox limit <ID> --per-day <COUNT>` | `skrzynka mailbox limit <ID> --clear` |
| HTTP | `GET /v1/mailboxes/:id/limit` | `PUT /v1/mailboxes/:id/limit` with `{"per_day": COUNT}` | `PUT /v1/mailboxes/:id/limit` with `{"per_day": null}` or `{}` |
| Desktop | **Send limit** in the mailbox inspector | **Set limit** | **Remove limit** |

Every surface answers the same object:

| Field | Meaning |
| --- | --- |
| `mailbox_id` | The mailbox. |
| `per_day` | The limit in force today; `null` when the mailbox has none. |
| `warm_up` | How the limit grows, `{from, grow_by, up_to, since, reaches_up_to_on}`; `null` for a limit that stays as set. |
| `day` | The current UTC day, `YYYY-MM-DD`. |
| `sent_today` | Sends claimed on `day` and not recorded `failed`. |
| `remaining` | What the limit still admits today; `null` without a limit. |

## What counts

Every originated message, reply and unsubscribe mail the mailbox claims
counts from the moment it is claimed, unless it ends `failed`. A one-click
unsubscribe sends no mail and does not count. Repeating a request with its
idempotency key returns the first attempt and is not counted again.

The limit is checked when a send is claimed, under a lock on the mailbox's
limit row, so two requests at the same time cannot both take the last place.
A send past the limit is refused before anything is recorded or sent:

| Code | HTTP | When |
| --- | --- | --- |
| `MAILBOX_SEND_LIMIT_REACHED` | 429 | The mailbox already sent its `per_day` messages today. The message names the count, the limit, the day and the command that raises it. |
| `NOT_FOUND` | 404 | The organization holds no mailbox with that ID. |

`--per-day` takes a positive whole number; Clap refuses zero or anything else
before dispatch. HTTP refuses such a body before the handler runs. Setting a
limit needs the organization's admin role; reading it needs any verified
role.

## Warming up

`--grow-by <COUNT> --up-to <COUNT>` beside `--per-day` (HTTP: `grow_by` and
`up_to` beside `per_day`; Desktop: **Grow by each day** and **Up to** above
**Set limit**) makes the limit grow by itself: `--per-day` today, `grow_by`
more on each UTC day after it, never past `up_to`, so a new address is
ramped up without the operator raising the limit every day. `per_day` then
reads the count in force today and `warm_up` says how it was set
(`since` is the day it began, `reaches_up_to_on` the UTC day the limit first
allows `up_to`); Desktop shows it as **Warm-up**. Setting the
limit again, with or without a warm-up, replaces it and starts any new
warm-up from that day; `--clear` removes both.

| Code | HTTP | When |
| --- | --- | --- |
| `MAILBOX_LIMIT_INVALID` | 400 | `up_to` is not above `per_day`, `grow_by` or `up_to` is given without the other over HTTP, or a warm-up names no `per_day` to start from. Nothing changes. |

Clap refuses `--grow-by` without `--per-day` and `--up-to`, and `--up-to`
without `--grow-by`, with exit 2.

## Real test

`tests/delivery/limit.mjs` runs the built CLI against a dedicated real mailbox
and its real SMTP account. It removes any limit and confirms it reads as
absent, sends one real message to the recipient the operator names and
confirms today's count rose, sets the limit to exactly that count and
confirms nothing remains, confirms the next new message is refused with
`MAILBOX_SEND_LIMIT_REACHED` and leaves no outbound row for its key, confirms
repeating the first message's key still answers the first attempt without
counting again, sets a warm-up from that count and confirms it reads back
as set, starts at that count today and admits nothing more, confirms a
ceiling equal to the start is refused with `MAILBOX_LIMIT_INVALID` and
`--grow-by` without `--up-to` is refused, confirms setting the limit alone
drops the warm-up, and finally restores the mailbox's prior limit and
warm-up.

```sh
node tests/delivery/limit.mjs --organization <ORG> --mailbox <ID> \
  --recipient <ADDRESS> --binary <PATH> --source-revision <COMMIT>
```

Reports remain under `.build/real-tests/mailbox-limit/<run>/report.json`.
