# Runbook

Start from the observable symptom. CLI errors are JSON on stderr with exit 1; HTTP errors are `{"error":{code,message,retryable}}`. Do not put message bodies, tokens, passwords, or Skarbiec payloads in an incident report.

## Service will not start

| Code / exact message | Meaning | Action |
|---|---|---|
| `NON_LOOPBACK_BIND_REFUSED` — `Skrzynka serves only loopback addresses` | `--bind` IP is not loopback | Use `127.0.0.1:<port>` or `[::1]:<port>`; do not proxy this as a remote shared API |
| `POLL_INTERVAL_INVALID` — `poll interval must be between 15 and 86400 seconds` | invalid process interval | Choose 15–86400 |
| `DATABASE_SCHEMA_UNSUPPORTED` — `database schema {n} is not supported by this build (expected at most 2)` | database is newer than this binary | Stop all processes; use a compatible build or restore the stopped-service pre-upgrade backup |
| `INTERNAL_ERROR` — `local state directory could not be created` | parent path cannot be created | Fix path ownership/permissions or choose `--database` elsewhere |
| `INTERNAL_ERROR` — `loopback API address could not be bound` | port/address is occupied or unavailable | Stop the conflicting process or choose another loopback port |
| `INTERNAL_ERROR` — `central identity URL is invalid` / `central identity configuration is incomplete` | environment is malformed or not HTTPS | Correct `SUPABASE_URL` and nonempty `SUPABASE_ANON_KEY` |

Do not run two Skrzynka processes against the same database. Back up the stopped database plus `-wal` and `-shm` companions.

## `/v1` returns no data

| HTTP/code | Exact message | Action |
|---|---|---|
| 401 `AUTHENTICATION_REQUIRED` | `a valid Wisent session is required` | Supply `Authorization: Bearer ...` and a valid `X-Wisent-Organization-ID`; refresh an expired session |
| 403 `ORGANIZATION_ACCESS_DENIED` | `the signed-in account does not belong to the selected organization` | Select an organization in the verified user's membership |
| 502 `IDENTITY_UNAVAILABLE` (`retryable:true`) | `central identity verification is unavailable` | Restore identity connectivity/configuration, then retry; do not bypass verification |
| 404 `NOT_FOUND` | `{resource} was not found` | Confirm UUID and selected organization; foreign resources intentionally look absent |
| 400 `INVALID_IDENTIFIER` | `identifier is not a valid UUID` | Send canonical UUID text |

`/healthz` can remain 200 while identity is unavailable; it proves process/schema readiness, not authenticated dependencies.

## Mailbox cannot be created or updated

| Code | Exact sentence / condition | Action |
|---|---|---|
| `MAILBOX_ALREADY_EXISTS` | `a mailbox already uses this Skarbiec item` | Use the existing mailbox or select another exact item |
| `CONFIRMATION_REQUIRED` | `mailbox removal requires --confirm` (CLI) / `mailbox removal requires confirm=true` (HTTP) | Reinspect scope, then explicitly confirm |
| `MAILBOX_PROFILE_INVALID` | `skarbiec_item_id must contain 1 to 256 non-whitespace characters` | Correct the ID |
| same | `email is required` / `email is not a valid address` | Supply a valid address explicitly or in the bundle |
| same | `imap_host is required` / `smtp_host is required` | Supply host or complete the bundle |
| same | `{field} must be a hostname without a URL scheme` | Use hostname only, no scheme or whitespace |
| same | `mail server ports must be nonzero` | Use valid nonzero ports |
| same | `poll_interval_seconds must be between 15 and 86400` | Correct mailbox interval |
| same | `display_name must contain between 1 and 200 characters` | Supply a bounded nonempty label |
| `GMAIL_PROFILE_INVALID` | `email is not a valid address` | Correct `gmail delegate --email` |

SMTP security accepts only `starttls` and `tls`; CLI rejects other values during parsing and JSON rejects an unknown enum.

## Skarbiec is unhealthy

| Code | Exact message | Action |
|---|---|---|
| `SKARBIEC_UNAVAILABLE` | `Skarbiec could not be started from the configured path` | Correct `--skarbiec-bin`, install it, and retry |
| same | `Skarbiec metadata listing failed` | Run the selected Skarbiec version and restore its local authority |
| `SKARBIEC_TIMEOUT` | `Skarbiec did not finish within 15 seconds` | Resolve vault/CLI stall; retry only after it is responsive |
| `SKARBIEC_RESPONSE_TOO_LARGE` | `Skarbiec response exceeded the 2 MiB safety limit` | Reduce/correct the selected output |
| `SKARBIEC_RESPONSE_INVALID` | `Skarbiec returned invalid metadata JSON` / `Skarbiec returned invalid item JSON` | Upgrade/fix Skarbiec contract output |
| `SKARBIEC_ITEM_INVALID` | `selected Skarbiec item is missing, unreadable, or unavailable` | Restore/select the exact item |
| same | `item has no canonical kind`, `item kind must be login or bundle`, or `item has no canonical fields object` | Rewrite as canonical `skarbiec.item.v2` |
| same | `item field {name} is required and must be text` | Add the named text field; never copy it into Skrzynka configuration |
| `SKARBIEC_WRITE_FAILED` | Gmail authorization could not be sent, persisted, or was rejected | Restore write authority and restart the Gmail connection flow |

A false `skarbiec_available` in `status` is diagnostic state, not a command failure. Existing local messages remain readable.

## Synchronization fails

| Code | Exact message | Action |
|---|---|---|
| `IMAP_UNAVAILABLE` (`retryable:true`) | `IMAP server could not be reached over TLS` | Verify host, port, DNS, TLS, and provider availability; note there is no configured IMAP network timeout |
| `IMAP_AUTHENTICATION_FAILED` | `IMAP authentication was refused; inspect the selected Skarbiec item` | Correct/rotate password or app password in Skarbiec |
| same | `Google refused the saved Gmail authorization; reconnect the profile` | Reconnect Gmail |
| `IMAP_INBOX_UNAVAILABLE` | `the provider did not make INBOX available` | Restore provider INBOX access |
| `IMAP_SEARCH_FAILED` | `IMAP UID search failed` | Inspect provider protocol state and retry |
| `IMAP_FETCH_FAILED` | `IMAP fetch failed at UID {uid}` | Retry later; cursor did not commit this failed batch |

All-mailbox sync exits 0 and embeds each failure. Inspect every `mailboxes[].ok`, plus mailbox `last_error_code`/`last_error_message`. A raw message over 2 MiB, missing body, or malformed content is counted as `skipped`; it does not produce a per-message API error.

## Reply fails or is ambiguous

| Code | Exact message | Action |
|---|---|---|
| `REPLY_FILE_INVALID` | `reply body file could not be read` | Correct path/permissions |
| same | `reply body file must be a regular file no larger than 256 KiB` | Use a regular bounded file |
| same | `reply body file must contain valid UTF-8 text` | Convert to UTF-8 |
| `IDEMPOTENCY_KEY_INVALID` | `idempotency_key must contain 1 to 200 non-whitespace characters` | Correct the key |
| `IDEMPOTENCY_KEY_REUSED` | `idempotency key already belongs to a different reply request` | Never repurpose a key; generate a new key only for a deliberate new send |
| `REPLY_BODY_INVALID` | `reply body must not be empty` | Add non-whitespace text |
| `REPLY_BODY_TOO_LARGE` | `reply body exceeds the 256 KiB limit` | Reduce body |
| `MESSAGE_REPLY_TARGET_INVALID` | `message has no valid Reply-To or From address` | Do not send; correct source data outside Skrzynka |
| `SMTP_TLS_FAILED` | `SMTP TLS configuration was rejected` | Correct host/security profile |
| `SMTP_REJECTED` | `SMTP explicitly rejected the reply; inspect mailbox status before retrying` | Fix provider/account condition; a new key is a new send |
| `SMTP_UNCERTAIN` / stored `REPLY_UNCERTAIN` | `SMTP acceptance is uncertain; inspect provider Sent mail before another attempt` | Inspect provider Sent mail; never auto-resend |

After an unclean stop, `send task stopped before terminal SMTP evidence was recorded` marks the attempt uncertain. Preserve the database while investigating.

## Gmail OAuth and delegation

| Code | Exact meaning / recovery |
|---|---|
| `GMAIL_OAUTH_CLIENT_INVALID` | OAuth authorization endpoint is invalid/untrusted; replace the Desktop-app client item with canonical endpoints |
| `GMAIL_OAUTH_STATE_MISSING` / `GMAIL_OAUTH_STATE_INVALID` | callback did not carry the issued state; start a new flow |
| `GMAIL_OAUTH_CODE_INVALID` | Google returned no valid code; start again |
| `GMAIL_OAUTH_FLOW_EXPIRED` — `Gmail authorization flow expired` | ten-minute flow elapsed; start again |
| `GMAIL_OAUTH_FLOW_CONSUMED` | flow is processing/completed or has no pending authorization; poll it or start a new flow |
| `GMAIL_OAUTH_REJECTED` | operator or Google rejected authorization; start again after correcting account policy |
| `GMAIL_REFRESH_TOKEN_MISSING` | no durable authorization returned; reconnect and consent again |
| `GMAIL_OAUTH_ACCOUNT_MISMATCH` | authorized email differs from selected profile; authorize the exact selected account |
| `GMAIL_AUTHORIZATION_EXPIRED` | saved refresh authorization was rejected; reconnect profile |
| `GMAIL_TOKEN_REFRESH_UNAVAILABLE`, `GMAIL_OAUTH_UNAVAILABLE`, `GMAIL_IDENTITY_UNAVAILABLE`, `GOOGLE_TOKEN_UNAVAILABLE` | Google dependency unavailable; retry only when `retryable:true` |
| `GMAIL_OAUTH_RESPONSE_INVALID`, `GMAIL_TOKEN_RESPONSE_INVALID` | Google returned a missing/invalid field; do not retry blindly; inspect provider status and version |
| `GOOGLE_DELEGATION_NOT_GRANTED` | follow the returned message exactly: grant its client ID and `https://mail.google.com/` in Workspace admin, then retry |
| `GOOGLE_DELEGATION_REJECTED` | verify the address is an active user in the Workspace domain that granted that client ID |

The browser callback deliberately shows only generic success/failure HTML. Obtain the exact code from the authenticated flow-status resource or CLI error, never from token-bearing browser URLs.