# Mailbox delivery report

`skrzynka mailbox delivery <ID>` (HTTP `GET /v1/mailboxes/:id/delivery`,
Skrzynka Desktop: **Delivery** in the mailbox inspector) answers how the
mailbox's sends ended. It reads only what Skrzynka already stored; it sends
nothing and asks no provider.

| Field | Meaning |
| --- | --- |
| `mailbox_id` | The mailbox asked about. |
| `since`, `until` | The window counted (RFC 3339, UTC), `null` for no bound. |
| `kinds[]` | One entry per kind of send with records: `kind`, `total`, `pending`, `sending`, `sent`, `failed`, `uncertain`, `bounced`, `refusals[]` (`code`, `count`). |
| `days[]` | One entry per UTC day a send was claimed on, oldest first, every kind together: `day` (`YYYY-MM-DD`), `total`, `sent`, `failed`, `uncertain`, `in_flight`, `bounced`. |
| `domains[]` | One entry per recipient domain of the originated mail claimed in the window, most sent first: `domain`, `sent`, `failed`, `bounced`, each counting recipients. |
| `bounces[]` | One entry per recipient outcome of a stored bounce report about one of the sends: `kind`, `send_id`, `report_message_id`, `received_at`, `recipient`, `action`, `status`, `diagnostic`, `unreadable`. |

## Counts

`kinds` holds one entry per kind of send the mailbox has records of:
`reply`, `outbound` (originated messages) and `unsubscribe` (unsubscribe
requests, one-click or mail). Each counts its records by status and each
refusal code the records carry, most frequent first. `sent` means the
provider's server accepted the request; nothing after acceptance is in the
status counts. `bounced` counts the distinct sends of that kind a stored
bounce report in `bounces` returned: set beside `sent`, it is the share
accepted that, as far as reports tell, did not arrive. Desktop shows it as
**returned by a bounce report** on each kind's line.

`days` counts the same records by the UTC day each was claimed on, so the
day totals add up to the kinds' totals; `in_flight` is `pending` and
`sending` together. A day's `bounced` counts the distinct sends the reports
received that day returned, whenever those sends went out, and a day on
which only reports arrived is listed with nothing claimed. Read beside a
[warm-up](limit.md), it shows whether a higher limit brought more bounces.
Desktop lists the days below the kinds, each as `YYYY-MM-DD (UTC)`.

`domains` takes the originated messages claimed in the window and counts
each To, Cc and Bcc recipient under the domain after its `@`: `sent` the
recipients of sends that ended `sent`, `failed` those of sends that ended
`failed`, and `bounced` the recipients an outbound bounce in `bounces`
says delivery to failed. A domain whose sends are all in flight or
uncertain is listed with nothing counted. Set side by side, they show which
receiving provider turns the mailbox's mail away. Desktop lists them as
**By recipient domain**.

`mailbox delivery <ID> … --days-csv <PATH>` or `--domains-csv <PATH>` (HTTP
`GET /v1/mailboxes/:id/delivery/days/csv` or `…/domains/csv` with the same
window, `text/csv`; Desktop: **Days as a table (.csv)…** and **Domains as a
table (.csv)…** in **Delivery**) writes the report's `days` (header `day,
total, sent, failed, uncertain, in_flight, bounced`) or `domains` (header
`domain, sent, failed, bounced`) to a new file as a CSV table (RFC 4180,
CRLF lines) in the report's order, and prints `{path, rows, size_bytes}`: a
warm-up, or the providers turning mail away, read in a spreadsheet. An
existing path is `DELIVERY_OUTPUT_EXISTS` before anything is read, one that
cannot be checked, created or written `DELIVERY_OUTPUT_INVALID`; the two
flags together are refused by Clap.

## Bounces

Every import reads whether a message is a delivery status notification
(RFC 3464: `multipart/report; report-type=delivery-status`). For one that is,
it records the `Message-ID` of the returned message and, for each recipient
block, `Final-Recipient`, `Action`, `Status` and `Diagnostic-Code` exactly as
the reporting server wrote them. A report whose parts cannot be read is kept
with `unreadable` naming which part and why, together with whatever was read
before it. `skrzynka message show <ID>` (and every message read) carries
the recorded report in the message's `bounce` field; Skrzynka Desktop shows
it as **Bounce report** in the message inspector.

`bounces` lists one row per recipient outcome of every stored report, in any
of the organization's mailboxes, whose returned `Message-ID` equals the one a
reply, originated message or unsubscribe mail of this mailbox went out with,
newest report first. A report that names no recipient is one row with
`recipient`, `action`, `status` and `diagnostic` all absent.

What the report cannot show:

- a bounce that has not been imported yet: import the mailbox that receives
  the reports first with `skrzynka sync --mailbox <ID>`;
- a report that returns no headers of the original message, since nothing
  names which send it is about;
- a send that was accepted and then filtered or placed in a spam folder,
  because nobody reports that back.

A message imported before bounce reports were read keeps no report; its
recorded details are not read again.

## Addresses that bounced

`skrzynka message bounced` (HTTP `GET /v1/bounced`; Desktop: **Addresses
that bounced** in the Outbound inspector while no message is selected) lists
every address a stored report says delivery to failed for good (`Action:
failed`), in address order: `{address, reports, last_report_id,
last_reported_at, status, diagnostic}`, the address taken from
`Final-Recipient` after its type (`rfc822;`) and in lower case, with how many
reports said so and what the newest said. `--since <WHEN>` (HTTP `since=`;
Desktop: **Reported since** with a date) keeps the addresses whose newest
report was received at or after an RFC 3339 instant, a `YYYY-MM-DD` day or
a span back such as `-7d`: what bounced lately; `--until <WHEN>` (HTTP
`until=`; Desktop: **Reported before**) those whose newest report came
before it: addresses that bounced long ago and may work again, to try with
`--clear`. `--contains <TEXT>` (HTTP `contains=`; Desktop:
**Address contains** with **Find**) keeps the addresses containing a text,
case aside, such as `@example.com` for one domain. An unreadable instant, a
window whose start is not before its end, or a blank text is
`MESSAGE_FILTER_INVALID`; none changes which sends are refused, which is
every listed address.

`--csv <PATH>` (HTTP `GET /v1/bounced/csv` with the same parameters,
`text/csv` offered as `bounced.csv`; Desktop: **Export as a table (.csv)…**
in **Addresses that bounced**) writes the addresses listed to a new file as
a CSV table (RFC 4180, CRLF lines) under the header `address, reports,
last_report_id, last_reported_at, status, diagnostic`, a status or
diagnostic the newest report did not state empty, and prints `{path,
addresses, size_bytes}`: the list a CRM or mailing tool takes in to stop
writing to them. The file is never overwritten: an existing path is
`BOUNCED_OUTPUT_EXISTS` before anything is read, and a path that cannot be
checked, created or written is `BOUNCED_OUTPUT_INVALID` with the cause.

A new send to such an address is refused before anything is claimed:
`message send`, `message forward` and `message draft send` answer
`OUTBOUND_RECIPIENT_BOUNCED` (409), naming each address with its newest
report, so mail that cannot arrive does not count against the mailbox with
providers. `--allow-bounced` (`allow_bounced: true` on the send, forward and
draft-send bodies; Desktop: **Send even to addresses that bounced** on
Compose, Forward and the draft editor) sends anyway, for an address known to
work again. A repeated idempotency key answers its first attempt whatever
was reported since. A scheduled draft is always held to the reports: one
refused this way is unscheduled with why. Replies are not held to them: a
reply answers mail that arrived from the address.

An address known to work again (the mailbox was fixed, the typo was the
other side's) is cleared with `skrzynka message bounced --clear <ADDRESS>`
(HTTP `DELETE /v1/bounced/:address`; Desktop: **Works again** beside it),
which answers `{address, cleared_at}`. The reports received until then no
longer count: the address leaves the list and sends to it are no longer
refused. A report received after it counts again. An address the list does
not hold is `BOUNCE_NOT_FOUND` (404), naming the ones it does.

`message bounced --clear-listed` with `--contains <TEXT>`, `--since` or
`--until` (HTTP `DELETE /v1/bounced?contains=&since=&until=`; Desktop: **All
listed work again…** once a filter is applied, after a confirmation) clears
every address that listing keeps at one instant, such as a whole domain
whose server is fixed, and answers each as `{address, cleared_at}`. Narrowed
by none of them it is `MESSAGE_FILTER_INVALID`, so nothing clears every
address at once; a listing keeping none is `BOUNCE_NOT_FOUND`.

## Refusals

| Code | When |
| --- | --- |
| `NOT_FOUND` | The organization holds no mailbox with that ID. |
| `MESSAGE_FILTER_INVALID` | `--since` or `--until` is neither an RFC 3339 instant, a `YYYY-MM-DD` day nor a span back from now such as `-7d`, or `since` is not before `until`; or `--clear-listed` is narrowed by nothing. |
| `OUTBOUND_RECIPIENT_BOUNCED` (409) | A send, forward or draft send names an address a stored report says failed for good, without `--allow-bounced`. |
| `BOUNCE_NOT_FOUND` (404) | `message bounced --clear` names an address the list does not hold, or `--clear-listed` keeps none. |
| `BOUNCED_OUTPUT_EXISTS`, `BOUNCED_OUTPUT_INVALID` | `message bounced --csv` names a path that exists, or one that cannot be checked, created or written. |
| `DELIVERY_OUTPUT_EXISTS`, `DELIVERY_OUTPUT_INVALID` | `mailbox delivery --days-csv` or `--domains-csv` names a path that exists, or one that cannot be checked, created or written. |

## A window

`--since <WHEN>` and `--until <WHEN>` (HTTP `since=`, `until=`; Desktop:
**Only since** and **Only before**, each with a date) count only the sends claimed at or after `since`
and before `until`, and list only the bounce reports received in it; either
may be left out. Each is an RFC 3339 instant, a `YYYY-MM-DD` day read as
its UTC start, or a span back from now such as `-7d`, as `message list
--since` takes them, so a day given as `--since` counts that day's sending
and everything after it, and `--since -7d` the last seven days.

## Real test

`tests/delivery/report.mjs` runs the built CLI against a dedicated real
mailbox that has already sent mail, compares every count with the rows
`message outbound --mailbox` lists, checks each kind's `bounced` equals the
distinct sends its `bounces` name, checks the `days` add up to the kinds,
run oldest first, count every outbound row on its UTC claim day and give
each day the distinct sends its reports returned, checks `domains` counts
every recipient of the rows by domain and status and every failed outbound
bounce recipient, most sent first, checks `--since` the newest row's instant
counts exactly the rows claimed from then on and keeps no earlier bounce,
checks a window whose `since` is not before its `until` is refused with
`MESSAGE_FILTER_INVALID`, checks `--days-csv` and `--domains-csv` write
exactly the report's days and domains in order and an existing path is
`DELIVERY_OUTPUT_EXISTS`, checks `message bounced` is in address order and
names only addresses the stored reports (`message list --bounces`) say
failed (a cleared one is left out), that `--since` the newest report's
instant keeps exactly the addresses reported then or later and `--until`
it exactly the others, that a backwards window and an unreadable
`--since` are `MESSAGE_FILTER_INVALID`, that `--contains` each listed
domain in upper case keeps exactly the addresses holding it and a blank one
is `MESSAGE_FILTER_INVALID`, that `--csv` writes that header and one row
per listed address with its count, newest report and instant, in order,
and refuses the same path again with `BOUNCED_OUTPUT_EXISTS`, leaving the
file as it was, that `--clear` of an address it does not
hold is refused with `BOUNCE_NOT_FOUND` and changes nothing, that
`--clear-listed` narrowed by nothing is `MESSAGE_FILTER_INVALID` and
narrowed to a text no address holds `BOUNCE_NOT_FOUND`, the list unchanged
(no address the operator holds is ever cleared by the test), and, when the
list holds one, that `message send` to it is refused with
`OUTBOUND_RECIPIENT_BOUNCED` and claims no row, and confirms an unknown
mailbox ID is refused with `NOT_FOUND`. It sends nothing and clears no
listed address, so the operator's list stays as it was.

```sh
node tests/delivery/report.mjs --organization <ORG> --mailbox <ID> \
  --binary <PATH> --source-revision <COMMIT>
```

Reports remain under `.build/real-tests/mailbox-delivery/<run>/report.json`
with every command, its exit status, stdout and stderr.
