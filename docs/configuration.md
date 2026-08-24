# Configuration

Skrzynka has no configuration file. Process settings come from CLI options or environment; each mailbox stores its own non-secret profile. Credentials remain Skarbiec items.

## Process options

| Reader | Key | Default | Validation / effect |
|---|---|---|---|
| global CLI | `--database PATH` | `$HOME/.local/share/skrzynka/skrzynka.db` | opens SQLite, creates parent directories |
| global CLI | `--skarbiec-bin PATH` | `skarbiec` | exact child executable |
| `serve` | `--bind SOCKET` | `127.0.0.1:8788` | IP must be loopback; also becomes Gmail callback base |
| `serve` | `--poll-seconds N` | `60` | 15–86400; status value and default for created mailboxes |
| message list | `--limit N` | `100` | clamped to 1–500 |
| message list | `--offset N` | `0` | database offset |

The scheduler itself wakes every 15 seconds; `--poll-seconds` is not that wake period. It is the default due interval stored on new mailboxes.

## Environment

| Variable | Default | Reader and behavior |
|---|---|---|
| `HOME` | current directory when absent | `default_database_path`; appends `.local/share/skrzynka/skrzynka.db` |
| `SUPABASE_URL` | repository's compiled shared-identity HTTPS URL | `AuthVerifier::from_environment`; must parse as HTTPS |
| `SUPABASE_ANON_KEY` | repository's compiled shared-identity anonymous key | same reader; trimmed value must be nonempty |
| `RUST_LOG` | `info` | `tracing_subscriber::EnvFilter::try_from_default_env`; controls stderr logs |

`SUPABASE_URL` and `SUPABASE_ANON_KEY` affect `/v1` authentication, not CLI `legacy-local` data access. Invalid identity setup prevents `AppState` construction even for local state commands other than `version`.

`SKRZYNKA_SOURCE_REVISION` is a **compile-time** variable read by `option_env!`, not runtime configuration. It changes only the `source` field of `skrzynka version`; absent means `source-build`.

## Mailbox create keys

The CLI names and HTTP JSON names map as follows:

| CLI | HTTP / stored key | Default / source |
|---|---|---|
| `--skarbiec-item` | `skarbiec_item_id` | required exact ID |
| `--display-name` | `display_name` | bundle, then email |
| `--email` | `email` | bundle, then username when it contains `@`; otherwise required |
| `--imap-host` | `imap_host` | bundle; otherwise required |
| `--imap-port` | `imap_port` | bundle, then 993 |
| `--smtp-host` | `smtp_host` | bundle; otherwise required |
| `--smtp-port` | `smtp_port` | bundle, then 587 for STARTTLS or 465 for TLS |
| `--smtp-security` | `smtp_security` | bundle, then `starttls`; enum `starttls|tls` |
| `--poll-seconds` | `poll_interval_seconds` | process value, normally 60; 15–86400 |

Explicit create values override bundle values. HTTP `PATCH /v1/mailboxes/{id}` accepts every non-secret stored key above except `skarbiec_item_id`, plus `enabled`. Patch fields replace existing values and are revalidated.

## Password item fields

Canonical item `kind` must be `login` or `bundle`. `fields` supports:

| Field | Required | Notes |
|---|---:|---|
| `username` | yes | authentication identity; may supply email |
| `password` | password auth | secret, resolved per connection |
| `email` | unless username is an address or explicit create value | outgoing address |
| `display_name` | no | 1–200 characters after trimming |
| `imap_host`, `smtp_host` | unless explicit create values | hostnames, not URLs |
| `imap_port`, `smtp_port` | no | integer or numeric text accepted |
| `smtp_security` | no | `starttls` or `tls` |

## Fixed Gmail item IDs

- `skrzynka-google-oauth-desktop`: `fields.value` contains a Google Desktop-app client JSON. The `installed` object must contain text `client_id` and `client_secret`; endpoints must be canonical.
- `skrzynka-google-service-account`: `fields.value` contains a Google `service_account` JSON with `client_email`, numeric `client_id`, `private_key`, optional `private_key_id`, and canonical token URI.
- `skrzynka-gmail-<digest>`: generated bundle for one OAuth or delegated mailbox. Do not hand-copy token values into process configuration.

## Fixed safety bounds

These are compiled behavior, not knobs: Skarbiec child timeout 15 seconds; Skarbiec stdout 2 MiB; IMAP batch 200 messages; raw message 2 MiB; reply 256 KiB; HTTP list cap 500; SQLite busy timeout 5 seconds; OAuth flow lifetime 10 minutes; SMTP timeout 30 seconds. The IMAP client does not configure a network timeout.