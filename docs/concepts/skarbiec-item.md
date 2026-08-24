# Skarbiec item

A Skarbiec item is the external credential authority selected by exact ID for one mailbox. Skrzynka persists the ID and non-secret connection profile; it does not persist the resolved secret.

## Supported shapes

Skrzynka accepts canonical `skarbiec.item.v2` items whose `kind` is `login` or `bundle` and whose `fields` is an object.

Password-backed mail needs text `username` and `password`. A complete bundle can also contain `email`, `display_name`, `imap_host`, `imap_port`, `smtp_host`, `smtp_port`, and `smtp_security`. Explicit mailbox-create values override these non-secret fields.

Gmail OAuth bundles use `auth_method: oauth2`, `oauth_provider: google`, `refresh_token`, and `oauth_client_item_id: skrzynka-google-oauth-desktop`. Delegated Gmail bundles use `auth_method: oauth2_service_account` and `service_account_item_id: skrzynka-google-service-account`. Both carry Gmail's fixed IMAP/SMTP profile.

## Child-process boundary

Skrzynka invokes the configured binary as:

- `skarbiec version` for availability;
- `skarbiec list` for non-secret item metadata;
- `skarbiec get <item-id>` immediately before profile or credential use;
- `skarbiec set-json <id> --type bundle` to persist Gmail authorization bundles.

Each child is bounded to 15 seconds and stdout is bounded to 2 MiB. Secret JSON stays inside the core process and connection task. It is never put in API resources, SQLite, or logs.

## Lifecycle

Password rotation is transparent because every sync and reply resolves the item again. Gmail refresh access tokens are cached in memory only until 60 seconds before expiry. Reconnecting OAuth rotates the durable Gmail bundle. Removing a Skrzynka mailbox leaves every Skarbiec item untouched.

## Refusals

- Invalid ID: `MAILBOX_PROFILE_INVALID` — `skarbiec_item_id must contain 1 to 256 non-whitespace characters`.
- Missing/unreadable item: `SKARBIEC_ITEM_INVALID` — `selected Skarbiec item is missing, unreadable, or unavailable`.
- Wrong kind or shape also uses `SKARBIEC_ITEM_INVALID`, with the exact missing field or shape sentence.
- Spawn/list failure: `SKARBIEC_UNAVAILABLE`.
- Deadline: `SKARBIEC_TIMEOUT` — `Skarbiec did not finish within 15 seconds`.
- Oversize: `SKARBIEC_RESPONSE_TOO_LARGE` — `Skarbiec response exceeded the 2 MiB safety limit`.
- Invalid JSON: `SKARBIEC_RESPONSE_INVALID`.
- Gmail persistence failure: `SKARBIEC_WRITE_FAILED`.

Skarbiec owns encryption, recovery, rotation, and revocation. Skrzynka owns only the reference and use boundary.