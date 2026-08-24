# Mailbox

A mailbox is Skrzynka's local description of one receive identity and its matching send identity. It belongs to one organization, points to exactly one Skarbiec item, reads `INBOX` over IMAP, and sends replies over SMTP.

## Shape

The public JSON resource contains:

| Field | Meaning |
|---|---|
| `id` | generated UUID |
| `skarbiec_item_id` | exact credential authority; globally unique in this database |
| `display_name`, `email` | human label and outgoing `From` address |
| `imap_host`, `imap_port` | direct-TLS IMAP endpoint |
| `smtp_host`, `smtp_port`, `smtp_security` | SMTP endpoint and `starttls` or `tls` mode |
| `poll_interval_seconds` | per-mailbox due interval, 15–86400 |
| `enabled` | whether all-mailbox and background sync consider it |
| `last_uid` | highest fetched UID whose batch was committed |
| `last_sync_at` | last successful commit time, or `null` |
| `last_error_code`, `last_error_message` | latest sync failure, or `null` |
| `created_at`, `updated_at` | RFC 3339 timestamps |

The stored `organization_id` is deliberately omitted from serialization. Passwords, authentication usernames, access tokens, refresh tokens, and service-account keys are not columns.

## Creation and precedence

`skarbiec_item_id` is required. Skrzynka accepts `login` and `bundle` items. For non-secret profile values, an explicit create argument wins over the bundle. It then uses bundle values, then these defaults:

- `display_name`: mailbox email;
- `imap_port`: `993`;
- `smtp_security`: `starttls`;
- `smtp_port`: `587` for `starttls`, `465` for `tls`;
- `poll_interval_seconds`: the process poll setting, normally `60`.

`email`, `imap_host`, and `smtp_host` must ultimately exist. A username containing `@` can supply the email. Creation validates the selected item and credential fields before inserting the row.

## Lifecycle

A mailbox starts enabled with `last_uid: 0`. Enable, disable, and updates change the same UUID. Successful synchronization advances the cursor and clears its error; failure records an error without advancing `last_sync_at`. Removal requires explicit confirmation and cascades through local messages and reply attempts. It does not change Skarbiec or the provider mailbox.

## Invariants and refusals

- One Skarbiec item maps to at most one mailbox: `MAILBOX_ALREADY_EXISTS` — `a mailbox already uses this Skarbiec item`.
- A missing row is indistinguishable across organizations: `NOT_FOUND` — `mailbox was not found`.
- `display_name` must contain 1–200 characters.
- Email must parse as an address.
- Hosts must be at most 253 characters, contain no whitespace, and contain no URL scheme.
- Ports must be nonzero.
- The poll interval must be 15–86400 seconds.
- Invalid profile data uses `MAILBOX_PROFILE_INVALID`; invalid global service polling uses `POLL_INTERVAL_INVALID`.

See [Skarbiec item](skarbiec-item.md), [synchronization](synchronization.md), and the [CLI reference](../cli.md).