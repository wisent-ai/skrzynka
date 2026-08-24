# Walkthrough: local mailbox lifecycle

This transcript was captured from the source-built 0.2.0 binary in an isolated temporary database. `--skarbiec-bin` pointed to a stand-in CLI that returned the canonical `skarbiec.item.v2` bundle for `team-inbox`; IMAP/SMTP ports were deliberately unreachable, so no operator state or provider was touched.

## 1. Inspect an empty database

```console
$ skrzynka --database "$STATE/skrzynka.db" status
{
  "product": "skrzynka",
  "version": "0.2.0",
  "database_path": "/var/folders/4m/g5zcy_y57jgfk_cg9dqt10w00000gn/T/skrzynka-docs.15ebyxek/state/skrzynka.db",
  "schema_version": 2,
  "mailbox_count": 0,
  "enabled_mailbox_count": 0,
  "message_count": 0,
  "poll_interval_seconds": 60,
  "skarbiec_available": true
}
```

The probe created the state directory and schema, but resolved no item.

## 2. Add the bundle

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

No secret is in the returned resource. Adding the same authority again proved the database invariant:

```console
$ skrzynka --database "$STATE/skrzynka.db" mailbox add --skarbiec-item team-inbox
{"error":{"code":"MAILBOX_ALREADY_EXISTS","message":"a mailbox already uses this Skarbiec item","retryable":false}}
```

## 3. Synchronize all enabled mailboxes

```console
$ skrzynka --database "$STATE/skrzynka.db" sync
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

The command exited 0 because all-mailbox synchronization completed and represented the per-mailbox failure in JSON. The mailbox retained the evidence:

```json
{
  "last_uid": 0,
  "last_sync_at": null,
  "last_error_code": "IMAP_UNAVAILABLE",
  "last_error_message": "IMAP server could not be reached over TLS"
}
```

A selected single-mailbox sync instead exited 1 with the standard retryable envelope:

```console
$ skrzynka --database "$STATE/skrzynka.db" sync --mailbox 3a9f65f0-a6e1-4520-bd35-708731a6a92f
{"error":{"code":"IMAP_UNAVAILABLE","message":"IMAP server could not be reached over TLS","retryable":true}}
```

## 4. Exercise state transitions

Disable and enable returned the same resource with `enabled:false` and then `enabled:true`; the UUID, cursor, and last error stayed intact. Removal without confirmation failed exactly:

```console
$ skrzynka --database "$STATE/skrzynka.db" mailbox remove 3a9f65f0-a6e1-4520-bd35-708731a6a92f
{"error":{"code":"CONFIRMATION_REQUIRED","message":"mailbox removal requires --confirm","retryable":false}}
```

With confirmation:

```console
$ skrzynka --database "$STATE/skrzynka.db" mailbox remove 3a9f65f0-a6e1-4520-bd35-708731a6a92f --confirm
{
  "removed": "3a9f65f0-a6e1-4520-bd35-708731a6a92f"
}
```

Only the isolated SQLite rows were removed; the stand-in item was untouched. For a real controlled inbox, continue with the [getting-started example](../examples/getting-started/add-and-sync-mailbox.sh).