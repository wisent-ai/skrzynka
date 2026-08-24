# Onboarding and first success

## First-success fact

For the CLI, `message list` returns at least one provider message whose `mailbox_id` equals the mailbox added from the selected Skarbiec item. For Skrzynka Desktop, the shared journey records `first_mailbox_sync_completed` only after the authenticated core completes a provider-backed synchronization cycle. Installation, sign-in, a healthy process, explanatory navigation, or a successful IMAP login alone is not first success.

## Zero state

`skrzynka help` performs no network access and creates no state. It explains the product, names Skarbiec and mailbox prerequisites, and gives the safe sequence: start locally, add one mailbox, synchronize, inspect messages. `skrzynka serve` may create the local state directory and database but refuses a non-loopback bind address.

## Machine journey

1. **Inspect:** `skrzynka status` reports database path, version, mailbox count, message count, and whether `skarbiec` can be found. It never resolves a credential.
2. **Select authority:** for a generic mailbox, choose one existing Skarbiec item by exact ID; for Gmail, Skrzynka Desktop lists deduplicated Google identities resolved from Skarbiec login metadata.
3. **Connect mailbox:** generic CLI setup validates the selected item and stores only non-secret fields plus its reference. Gmail setup runs a bounded PKCE authorization, writes a dedicated OAuth bundle to Skarbiec, creates the automatic Gmail profile, and stores only that bundle reference.
4. **Synchronize:** `skrzynka sync --mailbox MAILBOX_ID` opens IMAP over TLS using password authentication or Gmail XOAUTH2 and persists a bounded set of unseen UIDs.
5. **Observe success:** `skrzynka message list --mailbox MAILBOX_ID` shows the imported message.
6. **Reply when intended:** `skrzynka message reply MESSAGE_ID --body-file PATH` presents the provider-facing side effect before sending and returns terminal or ambiguous state.

The CLI is noninteractive. Automation receives one JSON object on stdout and a nonzero exit for failure; decorative human output is never required for parsing.

## Human desktop journey

Shared Wisent authentication first restores or establishes a real identity and selected organization. The private `skrzynka-desktop` client then runs the Echo-owned, version-bound `skrzynka-desktop.first-use` journey:

| Screen | Entry evidence | Completion evidence | Safe fallback |
|---|---|---|---|
| Welcome | Authenticated organization selected | Operator chooses the unified inbox workflow | Exit without mailbox state change |
| Service | Product promise understood | Authenticated `/v1/status` succeeds for the selected organization | Retry without advancing |
| Mailbox | Service ready | Core creates and reads back an organization-scoped mailbox from the chosen Skarbiec authority | Preserve selection and show the normalized error |
| Synchronize | Mailbox exists | Provider-backed synchronization completes | Retry without duplicating rows or provider effects |

The canonical journey version is `2026-08-11.1`, immutable version ID `7f1d3482-fd49-4c7c-9fb2-22d57e2acb60`. The desktop runs it through Echo's shared `WisentOnboarding` package, persists version-bound progress and idempotent events locally, and uses the validated bundled definition when the central transport is unavailable. When the Stado integration transport is configured, bundle reads and event collection use the centralized Echo onboarding boundary. Analytics failure never blocks the core mailbox workflow.

## Common failures

| Failure | Meaning | Recovery |
|---|---|---|
| `SKARBIEC_UNAVAILABLE` | CLI missing or cannot run | Install/start the supported Skarbiec release and re-run status |
| `SKARBIEC_ITEM_INVALID` | Item missing, unreadable, or lacks required credential fields | Correct the exact item or select another; no mailbox row is created |
| `GMAIL_OAUTH_REJECTED` | Google or the operator rejected the bounded authorization | Start a fresh connection flow; no partial authorization is used |
| `GMAIL_OAUTH_ACCOUNT_MISMATCH` | Google returned a different account than the selected Skarbiec identity | Authorize the exact selected address |
| `GMAIL_AUTHORIZATION_EXPIRED` | Google revoked or rejected the saved refresh token | Reconnect the same Gmail profile; local messages remain readable |
| `AUTHENTICATION_REQUIRED` | The Wisent session is absent or invalid | Return to shared sign-in; no organization data is returned |
| `ORGANIZATION_ACCESS_DENIED` | The selected organization is not one of the signed-in account's memberships | Select an authorized organization; no mailbox state is read or changed |
| `IDENTITY_UNAVAILABLE` | Central identity verification could not complete | Retry after the identity service recovers; the core fails closed |
| `MAILBOX_PROFILE_INVALID` | Required host/address/security setting is absent or contradictory | Supply non-secret settings or update the bundle |
| `IMAP_AUTHENTICATION_FAILED` | Provider refused the current credential | Rotate or correct the Skarbiec item, then retry sync |
| `IMAP_UNAVAILABLE` | TLS, DNS, connection, or provider failure | Inspect mailbox status and retry; other mailboxes continue |
| `SMTP_REJECTED` | Provider rejected the reply before acceptance | Correct the provider/account condition and use a new idempotency key |
| `REPLY_UNCERTAIN` | Process stopped after send began without terminal evidence | Inspect provider Sent mail; no automatic resend occurs |

## Reset and removal

`mailbox remove` deletes the local mailbox, its local messages, and reply records after explicit confirmation. It never deletes the Skarbiec item or provider mailbox. To reset the whole local product, stop Skrzynka and remove the configured database plus SQLite companion files; this is destructive to local normalized mail and reply evidence only.
