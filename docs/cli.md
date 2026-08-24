# CLI reference

`skrzynka` is a noninteractive JSON CLI. Successful commands print one pretty-printed JSON value to stdout and exit 0. Application failures print one compact envelope to stderr and exit 1:

```json
{"error":{"code":"MAILBOX_PROFILE_INVALID","message":"imap_host is required","retryable":false}}
```

Argument parsing failures are Clap text and exit 2. Logs go to stderr. The CLI operates in the fixed `legacy-local` organization namespace.

## Global options

| Option | Default | Meaning |
|---|---|---|
| `--database <PATH>` | `$HOME/.local/share/skrzynka/skrzynka.db` | SQLite state; parent directories are created |
| `--skarbiec-bin <PATH>` | `skarbiec` | executable invoked for credential operations |
| `-h`, `--help` | — | help; no state or network access |
| `-V`, `--version` | — | package version text from Clap |

Global options may appear with subcommands. `skrzynka version` instead emits the machine JSON object `{product, version, source}` and opens no database.

## Service and inspection

### `skrzynka serve`

```text
skrzynka serve [--bind <SOCKET>] [--poll-seconds <SECONDS>]
```

| Option | Default | Rule |
|---|---:|---|
| `--bind` | `127.0.0.1:8788` | address IP must be loopback |
| `--poll-seconds` | `60` | 15–86400; default for new mailboxes and status output |

Creates/opens the database, starts the 15-second due-check loop, and serves the [HTTP API](API.md). It remains in the foreground. Startup readiness is the stderr log `Skrzynka API ready address=...`.

### `skrzynka status`

Returns `{product, version, database_path, schema_version, mailbox_count, enabled_mailbox_count, message_count, poll_interval_seconds, skarbiec_available}`. The Skarbiec probe runs `skarbiec version` but resolves no item.

### `skrzynka version`

Returns `{product:"skrzynka", version, source}`. Source builds report `source-build` unless the compile-time `SKRZYNKA_SOURCE_REVISION` was set.

## Mailboxes

### `mailbox add`

```text
skrzynka mailbox add --skarbiec-item <ID>
  [--display-name <TEXT>] [--email <ADDRESS>]
  [--imap-host <HOST>] [--imap-port <PORT>]
  [--smtp-host <HOST>] [--smtp-port <PORT>]
  [--smtp-security starttls|tls] [--poll-seconds <SECONDS>]
```

Resolves and validates the exact Skarbiec item, merges explicit non-secret values over bundle values, inserts an enabled mailbox, and returns the complete public mailbox resource. `--skarbiec-item` is the only always-required option.

### Inventory and state

| Command | Output / effect |
|---|---|
| `mailbox list` | JSON array sorted case-insensitively by display name, then email |
| `mailbox show <UUID>` | one mailbox object |
| `mailbox enable <UUID>` | set `enabled:true`; return object |
| `mailbox disable <UUID>` | set `enabled:false`; return object |
| `mailbox remove <UUID> --confirm` | cascade-delete local mailbox state; return `{"removed":"<UUID>"}` |

Removal without `--confirm` is `CONFIRMATION_REQUIRED`. It never deletes provider or Skarbiec state.

## Synchronization

```text
skrzynka sync [--mailbox <UUID>]
```

With `--mailbox`, runs one pass and returns `{mailbox_id, received, skipped, last_uid, completed_at}`; a dependency failure exits 1. Without it, processes every enabled mailbox and returns `{completed_at, mailboxes:[...]}`. Individual failures are embedded as `{mailbox_id, ok:false, summary:null, error_code, error_message}` and the command still exits 0.

## Messages and replies

### `message list`

```text
skrzynka message list [--mailbox <UUID>] [--limit <N>] [--offset <N>]
```

Defaults: limit 100 and offset 0. Limits are clamped to 1–500. Returns newest imported messages first, optionally for one mailbox.

### `message show <UUID>`

Returns one message object. Invalid UUID syntax is rejected by Clap before the application; an absent UUID returns `NOT_FOUND`.

### `message reply`

```text
skrzynka message reply <MESSAGE-UUID> --body-file <PATH>
  [--idempotency-key <KEY>]
```

The file must be regular, UTF-8, and no larger than 256 KiB. Omitting the key generates a new UUID. Returns the reply attempt on SMTP acceptance or when an identical key already exists. Provider and validation failures use the standard error envelope; persisted attempt state remains available through the HTTP reply list.

## Gmail Workspace delegation

| Command | Output / effect |
|---|---|
| `gmail delegation` | `{configured, service_account, client_id, scope, admin_console_url}`; missing key is `configured:false`, not an error |
| `gmail delegate --email <ADDRESS> [--display-name <TEXT>]` | prove delegated access, persist the mailbox bundle in Skarbiec, and return a new or existing mailbox |

Interactive Gmail OAuth is HTTP/desktop-only; the CLI exposes Workspace delegation only.

## Exit and retry contract

- **0:** command's local contract completed. For all-mailbox sync, inspect each `ok`.
- **1:** normalized application error. Retry only when `retryable:true`, and never automatically resend an ambiguous reply.
- **2:** command line could not be parsed.

See [runbook](runbook.md) for every stable error family.