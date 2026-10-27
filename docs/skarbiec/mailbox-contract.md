# Skarbiec mailbox contract

For password-backed providers, Skrzynka persists the exact item selected by the operator. A complete Skarbiec `bundle` uses these fields:

| Field | Required | Meaning |
|---|---:|---|
| `username` | yes | Authentication identity for the protocol using this item |
| `password` | password auth | App password or provider-issued mailbox secret |
| `email` | yes | Default address used in the outgoing `From` header |
| `imap_host` | yes | IMAP TLS hostname |
| `imap_port` | no | Defaults to the TCP port `/etc/services` registers for `imaps` (993); a database that cannot be read, or that registers no such port, refuses the item by name |
| `smtp_host` | yes | SMTP hostname |
| `smtp_port` | no | Defaults to `587` for STARTTLS or `465` for implicit TLS |
| `smtp_security` | no | `starttls` (default) or `tls` |
| `display_name` | no | Human-readable mailbox name |

Skarbiec stores the account list, the account profiles, and the credentials: the tag `skrzynka:mailbox` declares an item a mailbox. `mailbox declare` accepts only `--skarbiec-item`; CLI profile flags and API profile overrides are refused. The fleet database keeps the imported non-secret snapshot and mail-processing state, not a separately editable account definition. Every reconciliation adopts source display-name and SMTP changes (`mailbox_state: updated` on declare). A changed receiving address or IMAP endpoint is refused because the retained UID cursor cannot safely identify another source.

Each declared profile has a receiving `skarbiec_item_id` and may contain `smtp_skarbiec_item_id` for a separate sending credential. Gmail connection methods persist complete bundles in Skarbiec and declare them there. OAuth stores its refresh token in Skarbiec; delegation stores a service-account reference. No mailbox secret crosses the desktop API or enters the database.
An absent or JSON-null `smtp_skarbiec_item_id` uses the receiving credential.
A present non-null value must be nonblank text naming a valid item; malformed
sending declarations refuse by field name instead of silently switching identities.

Credential writes feed Skarbiec's stdin while concurrently draining its stdout
and stderr. A child that reports a refusal before consuming the payload cannot
hold the caller on a full pipe. A process refusal preserves its status and cause;
a failed child wait reports that persisted state is unknown, rather than
asserting that nothing was saved. Full input delivery and a successful child
exit are both required before Skrzynka continues.

Refresh refusals do not all mean expired authorization. Only Google's
`invalid_grant` maps to `GMAIL_AUTHORIZATION_EXPIRED` and reconnect guidance.
Other JSON refusals retain their HTTP status, OAuth error and error description
under `GMAIL_TOKEN_REFRESH_UNAVAILABLE`; server errors and rate limiting remain
retryable. Token response payloads are not copied wholesale into diagnostics.
Delegation refusals likewise retain the provider's status, OAuth error and error
description alongside grant guidance. Key-decoding and assertion-signing failures
keep their actual library cause; the private key and signed assertion are not
included in those messages.
An undecodable token response still preserves the observed server-error or
rate-limit retry classification. Invalid OAuth-client and service-account JSON
reports the parser's cause and location without copying credential contents.

## The Google OAuth client and service account

Skrzynka asks Skarbiec for its Google credentials by the role they play, never
by an item ID: `skarbiec get role:<role>` reads the one live item tagged
`stado:role:<role>`.

The OAuth client is the item playing `skrzynka-google-oauth-desktop`: kind
`stado-secret`, with one `value` field of type `oauth_client` holding the
unmodified JSON downloaded for a Google OAuth client whose application type is
**Desktop app**. Skrzynka accepts only the `installed` client shape and
Google's canonical authorization and token endpoints.

**Desktop app** is not a preference: Google grants installed-app clients the
loopback exemption, so any loopback port is accepted without registering each
one, and one Desktop client serves every `--bind` port Skrzynka is given and
any other loopback flow of the same project at the same time. A Web-type client
has no such exemption and refuses every unregistered redirect URI with
`redirect_uri_mismatch` before any consent screen appears.

The service account used for domain-wide delegation is the item playing
`skrzynka-google-service-account`, holding the service account's JSON key in
the same `value` field.
