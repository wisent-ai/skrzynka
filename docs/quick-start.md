# Quick start

From a clone to a synchronized local inbox. Every command and output below
was executed against version `0.2.0` in an isolated state directory, with
`--skarbiec-bin` pointed at a stand-in Skarbiec CLI that serves one
documented `bundle` item (`team-inbox`) — the same JSON shapes a real
Skarbiec emits. Your IDs and timestamps will differ. The full command
surface is [cli](cli.md); the flag-free operator journey is
[onboarding](ONBOARDING.md).

## Build

Prerequisites: Rust 1.82+, a `skarbiec` executable, and a mailbox with IMAP
over TLS and SMTP over STARTTLS or implicit TLS.

```bash
git clone https://github.com/wisent-ai/skrzynka.git
cd skrzynka
cargo build
./target/debug/skrzynka version
```

```json
{
  "product": "skrzynka",
  "source": "source-build",
  "version": "0.2.0"
}
```

Every command prints one JSON object on stdout and exits `0`, or one
normalized error envelope on stderr and exits nonzero — automation never
parses decorative text.

## Inspect the zero state

`status` creates the state directory and database if missing, counts local
resources, and probes whether `skarbiec` can be started. It resolves no
credential and touches no provider.

```console
$ skrzynka --database "$STATE/skrzynka.db" status
{
  "product": "skrzynka",
  "version": "0.2.0",
  "database_path": "/var/folders/.../skrzynka-docs.15ebyxek/state/skrzynka.db",
  "schema_version": 2,
  "mailbox_count": 0,
  "enabled_mailbox_count": 0,
  "message_count": 0,
  "poll_interval_seconds": 60,
  "skarbiec_available": true
}
```

Without `--database` the default is `~/.local/share/skrzynka/skrzynka.db`;
without `--skarbiec-bin` the default is `skarbiec` on `PATH`
([configuration](configuration.md)).

## Add one mailbox

Select one Skarbiec item by exact ID. A complete `bundle` supplies
everything; only non-secret fields are persisted:

```console
$ skrzynka --database "$STATE/skrzynka.db" mailbox add --skarbiec-item team-inbox
{
  "id": "3a9f65f0-a6e1-4520-bd35-708731a6a92f",
  "skarbiec_item_id": "team-inbox",
  "display_name": "Team",
  "email": "team@example.invalid",
  "imap_host": "127.0.0.1",
  "imap_port": 8993,
  "smtp_host": "127.0.0.1",
  "smtp_port": 8587,
  "smtp_security": "starttls",
  "poll_interval_seconds": 60,
  "enabled": true,
  "last_uid": 0,
  "last_sync_at": null,
  "last_error_code": null,
  "last_error_message": null,
  "created_at": "2026-08-24T23:09:17.731786+00:00",
  "updated_at": "2026-08-24T23:09:17.731786+00:00"
}
```

No password or username appears — the row references the item, and the
secret is resolved only while a connection is open. A bare `login` item
needs the server profile on the command line
(`--email --imap-host --smtp-host` and ports/security as applicable). One
Skarbiec item maps to at most one mailbox; adding it again is refused with
`MAILBOX_ALREADY_EXISTS`.

## Synchronize and read

```bash
skrzynka --database "$STATE/skrzynka.db" sync
skrzynka --database "$STATE/skrzynka.db" message list
```

Against a reachable mailbox, `sync` prints one result per enabled mailbox
with a `summary` of `received`, `skipped`, and the advanced `last_uid`
cursor, and `message list` shows the normalized messages (sender, subject,
snippet, plain-text body). This isolated lab deliberately has no reachable
IMAP endpoint, so the same commands return the exact failure shape you will
meet in real operations — per-mailbox, normalized, and non-fatal to other
mailboxes:

```json
{
  "completed_at": "2026-08-24T23:09:24.159599+00:00",
  "mailboxes": [
    {
      "mailbox_id": "3a9f65f0-a6e1-4520-bd35-708731a6a92f",
      "ok": false,
      "summary": null,
      "error_code": "IMAP_UNAVAILABLE",
      "error_message": "IMAP server could not be reached over TLS"
    }
  ]
}
```

The failure is also recorded on the mailbox (`last_error_code`,
`last_error_message`) where `mailbox show <id>` and the desktop find it.
Every code and its fix: [runbook](runbook.md).

## Serve the loopback API

```console
$ skrzynka --database "$STATE/skrzynka.db" serve --bind 127.0.0.1:8791
2026-08-24T23:09:52.159767Z  INFO skrzynka: Skrzynka API ready address=127.0.0.1:8791
```

`serve` starts the poll loop (every mailbox on its own interval, default 60
seconds) and the authenticated JSON API used by `skrzynka-desktop`. Only
loopback binds are accepted. Health needs no session:

```console
$ curl -s http://127.0.0.1:8791/healthz
{"status":"ready","product":"skrzynka","version":"0.2.0","schema_version":2}
```

Everything under `/v1` requires a Wisent bearer and organization header and
fails closed without them ([API](API.md),
[walkthrough](walkthrough-loopback-api.md)).

## Reply when you mean it

```bash
skrzynka --database "$STATE/skrzynka.db" message reply <message-uuid> --body-file reply.txt
```

The reply goes out through the mailbox that received the message, as a
plain-text `Re:` with thread headers preserved. The body file must be UTF-8
and at most 256 KiB. Omitting `--idempotency-key` generates a fresh UUID;
supply your own key when a retrying script must never double-send
([reply attempt](concepts/reply-attempt.md)).

## Clean up

```bash
skrzynka --database "$STATE/skrzynka.db" mailbox remove <mailbox-uuid> --confirm
```

Removal cascades through local messages and reply records only; the
Skarbiec item and the provider mailbox are untouched. The captured
end-to-end sequence, including every refusal on the way, is
[walkthrough-local-lifecycle](walkthrough-local-lifecycle.md).
