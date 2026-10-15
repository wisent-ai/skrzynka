# Inbound domains

An inbound domain is a domain whose every address receives mail at a
receiving provider, such as the domains Weles signs accounts up on. Skrzynka
reads that mail through wisent-integrations
(`content/resend.receiving.list` and `content/resend.receiving.get`), stores
each message once by the provider's own id, and answers it by recipient and
sender, as it answers a mailbox's mail. Skrzynka holds no provider key.

## Setup

1. Skrzynka's wisent-integrations grant is the Skarbiec item
   `skrzynka-integration-api`, tagged `stado:role:skrzynka-integration-api`
   so Skrzynka asks for it by its role, with two fields: `token`, Skrzynka's
   integration bearer, and `url`, the wisent-integrations address. On the
   wisent-integrations side, the client policy entry for that item allows
   exactly `content/resend.receiving.list` and `content/resend.receiving.get`
   (wisent-integrations `docs/actions.md`, Skrzynka inbound domains).
2. Declare the domain for the organization:

```bash
skrzynka --organization ORGANIZATION mailbox declare --inbound-domain DOMAIN
```

Declaring the same domain again answers the same declaration.

## Reading an address's mail

```bash
skrzynka --organization ORGANIZATION message list --recipient ADDRESS --sender TEXT
```

When `ADDRESS` is at a declared inbound domain, Skrzynka first reads the
receiving feed for that domain and stores every message it has not stored,
then lists the address's stored mail, newest first. Each row carries `id`,
`domain_id`, `provider_id`, `sender`, `recipients`, `subject` (`null` when the
message states none; Desktop shows `No subject stated`), `received_at`
and `body_text` (the plain text part, or the HTML part when the message has no
plain text). `--sender` keeps the messages whose From contains the text;
`--limit` and `--offset` page the result. An address at any other domain is
answered from the mailboxes, as `docs/messages/filters.md` describes.

The feed is read newest first until a message of the domain that is already
stored; new messages are then stored oldest first. A read that fails part way
keeps what it stored, and the next read fills in the rest.

## Every surface

| Action | CLI | API | Desktop |
|---|---|---|---|
| Declare | `mailbox declare --inbound-domain DOMAIN` | `POST /v1/inbound/domains` `{"domain": …}` | Mailboxes, Inbound domains, Declare |
| List | `mailbox list --inbound` | `GET /v1/inbound/domains` | Mailboxes, Inbound domains |
| Read the feed | `sync --inbound-domain ID` | `POST /v1/inbound/domains/ID/sync` | Read feed |
| Read an address | `message list --recipient ADDRESS` | `GET /v1/inbound/messages?recipient=&sender=&limit=` | Read mail |
| Remove | `mailbox remove --inbound ID --confirm` | `DELETE /v1/inbound/domains/ID?confirm=true` | Remove |

Each domain row keeps `last_sync_at` and, after a failed read,
`last_error_code` and `last_error_message`: a failed read is recorded and
refused, never answered as no mail.

## Refusals

| Code | Cause |
|---|---|
| `INBOUND_DOMAIN_INVALID` | The value is not a mail domain; the parser's cause follows. |
| `INBOUND_DOMAIN_NOT_DECLARED` | The API was asked for an address whose domain the organization has not declared; the sentence names the `mailbox declare --inbound-domain` command. |
| `INBOUND_RECIPIENT_INVALID` | The API recipient is not a mail address. |
| `MESSAGE_FILTER_INVALID` | No recipient was named, or a filter is blank. |
| `INBOUND_FEED_UNCONFIGURED` | The Skarbiec item playing `skrzynka-integration-api` lacks `token` or `url`. |
| `INBOUND_FEED_UNAVAILABLE` | wisent-integrations could not be reached or its answer could not be read; retryable. |
| `INBOUND_FEED_REFUSED` | wisent-integrations refused the call or answered an unexpected shape; its status and body follow. |
| `CONFIRMATION_REQUIRED` | Removal without `--confirm` (`confirm=true`): the received mail is deleted and cannot be restored. |

## Proof

`tests/inbound/domain.mjs` runs the real CLI against an address that has
received mail at the receiving provider: it declares the address's domain,
reads the address through `message list --recipient`, checks every row is
addressed to it and stored under the domain, checks the read is recorded on
the domain, checks a second read stores nothing new and answers the same
messages, checks the sender filter, and checks the refusals for an invalid
domain, an unknown domain id and an unconfirmed removal. A domain the run
declared is removed at the end.

```bash
node tests/inbound/domain.mjs --organization ORGANIZATION --recipient ADDRESS \
  --binary target/debug/skrzynka --source-revision HEAD
```
