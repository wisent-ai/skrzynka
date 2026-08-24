# What is Skrzynka

Skrzynka is a self-hosted email operations service: one local process that
receives mail from any number of IMAP mailboxes into a single local inbox and
sends replies back out through the exact mailbox that received each message.
The whole product is three moving parts: a credential boundary that keeps
every secret in Skarbiec, a bounded synchronization loop that imports
provider mail into SQLite, and a reply state machine that treats every send
as an idempotency-keyed provider mutation.

## Credentials stay in Skarbiec

Skrzynka never stores a mailbox password. A mailbox row contains the exact
Skarbiec item ID the operator selected plus non-secret connection settings
(address, hosts, ports, security mode) — nothing else. When a
synchronization or reply actually needs to authenticate, the core resolves
that one item by running the local `skarbiec` CLI as a child process
(`skarbiec get <item-id>`, bounded to 15 seconds and 2 MiB of output), uses
the secret inside the connection task, and drops it with the task. No API
response, database column, or log line carries a password, OAuth code, or
provider token. Gmail is the same boundary with a different secret: an OAuth
refresh token or a domain-wide service-account key, written back to Skarbiec
as a dedicated `skrzynka-gmail-*` bundle and referenced by ID like any other
item. Details: [skarbiec item](concepts/skarbiec-item.md),
[Gmail connection](concepts/gmail-connection.md).

## Synchronization is bounded and resumable

Each enabled mailbox carries a cursor: the highest IMAP UID already
persisted. A sync opens one IMAP session over required TLS, searches
`UID <cursor+1>:*` in INBOX, imports at most 200 messages of at most 2 MiB
each, normalizes them to plain text, and inserts them keyed by
`(mailbox_id, UID)` — re-syncing the same UID is a no-op, so restoring a
backup never duplicates rows. The cursor and `last_sync_at` advance only
after the batch is committed; a failure instead records a normalized
`last_error_code`/`last_error_message` on the mailbox and leaves every other
mailbox running. `skrzynka serve` repeats this on a per-mailbox poll
interval (default 60 seconds); the CLI `skrzynka sync` runs one bounded pass
on demand. Nothing Skrzynka does mutates provider state: no flags, no
folders, no deletion. Details: [synchronization](concepts/synchronization.md),
[message](concepts/message.md).

## Replies are explicit provider mutations

A reply belongs to one stored inbound message and goes out through that
message's own mailbox — its Skarbiec item, its SMTP host, its `From`
address, with `In-Reply-To`/`References` preserved so threads stay intact.
Every reply request carries an idempotency key: the same key returns the
existing attempt and never sends twice. The attempt walks
`pending → sending → sent | failed | uncertain`; `sent` means the configured
SMTP server accepted the message, and `uncertain` means the process lost the
terminal SMTP evidence — Skrzynka refuses to resend automatically because
the provider may already have accepted it. Retrying after `failed` or
`uncertain` requires a new key, which is a deliberate new mutation.
Details: [reply attempt](concepts/reply-attempt.md).

## What Skrzynka is not

Skrzynka does not manage provider-side folders, delete or flag messages,
download attachments, render remote HTML, generate reply text, or send
automatic replies. It is not a shared server: the JSON API refuses any
non-loopback bind address, and every `/v1` request is verified against
central Wisent identity with organization-scoped database access
([organization scope](concepts/organization-scope.md)). It is not a
credential store — Skarbiec owns encryption, rotation, and revocation — and
it is not the mail authority: the provider owns the mailbox, the UIDs, and
delivery.

## The first three commands

```bash
skrzynka status
```

Database path, schema version, mailbox/message counts, poll interval, and
whether `skarbiec` can be started. No credential is resolved.

```bash
skrzynka mailbox add --skarbiec-item <item-id>
```

Validate the selected Skarbiec item and persist a mailbox that references
it. Only non-secret profile fields are stored.

```bash
skrzynka sync && skrzynka message list
```

One bounded import, then the normalized local inbox. The end-to-end path is
[quick-start](quick-start.md); the full command surface is [cli](cli.md).

## The rest of the corpus

- **Nouns** — [mailbox](concepts/mailbox.md), [message](concepts/message.md),
  [reply attempt](concepts/reply-attempt.md),
  [synchronization](concepts/synchronization.md),
  [skarbiec item](concepts/skarbiec-item.md),
  [organization scope](concepts/organization-scope.md),
  [Gmail connection](concepts/gmail-connection.md).
- **Interfaces** — [CLI reference](cli.md), [loopback JSON API](API.md).
- **Executed end to end** — [local mailbox lifecycle](walkthrough-local-lifecycle.md),
  [the loopback API from zero](walkthrough-loopback-api.md),
  runnable [examples](../examples/README.md).
- **When it fails** — every error code and exact sentence, with meaning and
  fix: [runbook](runbook.md).
- **Boundaries** — what Skrzynka owns, refuses to own, and which network
  edges exist: [architecture](architecture.md); every knob and default:
  [configuration](configuration.md).
- **Contracts** — [product](PRODUCT.md), [core behavior](CORE.md),
  [integrations](INTEGRATIONS.md), [onboarding](ONBOARDING.md),
  [release](RELEASE.md).
