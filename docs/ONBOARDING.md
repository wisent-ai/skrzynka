# Onboarding and first success

## First-success fact

`message list` returns at least one provider message whose `mailbox_id` equals the mailbox added from the selected Skarbiec item. Installation, a healthy process, or a successful IMAP login alone is not first success.

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

The private `skrzynka-desktop` client owns a deterministic, resumable multi-screen journey:

| Screen | Entry evidence | Completion evidence | Safe fallback |
|---|---|---|---|
| Welcome | No completed attempt | Operator chooses to manage existing mail | Exit without state change |
| Service | API health unknown or unavailable | `/healthz` reports `ready` | Show exact local start command |
| Skarbiec / Gmail | Service ready, no mailbox | A Google identity or exact generic item is selected | Gmail uses the discovered account list; generic setup filters metadata |
| Authorization / profile | Selection made | Gmail OAuth completes or the generic profile is accepted | Preserve selection and show the normalized error without storing partial state |
| First sync | Mailbox exists | At least one message is observed, or an empty provider inbox is explicitly acknowledged | Retry without duplicating rows |
| Access boundary | Before normal inbox | Operator sees that the current internal desktop has no price, checkout, or paid entitlement and continues | Exit; core CLI remains usable |
| Inbox | First-sync decision complete | Inbox is usable | Resume here on relaunch |

The current product has no paid capability, price, checkout, or entitlement. The access-boundary screen states that truth instead of inventing a purchase. If a paid capability is introduced, the README and release contract change first and this node becomes an authoritative paywall before activation.

The bundled journey is `skrzynka-desktop.first-success` version 1. Routing is deterministic from service reachability, mailbox count, selected item, and first-sync evidence. Progress is local and contains no credentials, message content, or unrestricted answers. There is no remote analytics or experiment assignment in version 1; the canonical control route is the only eligible variant, and analytics failure is therefore not a routing dependency.

## Common failures

| Failure | Meaning | Recovery |
|---|---|---|
| `SKARBIEC_UNAVAILABLE` | CLI missing or cannot run | Install/start the supported Skarbiec release and re-run status |
| `SKARBIEC_ITEM_INVALID` | Item missing, unreadable, or lacks required credential fields | Correct the exact item or select another; no mailbox row is created |
| `GMAIL_OAUTH_REJECTED` | Google or the operator rejected the bounded authorization | Start a fresh connection flow; no partial authorization is used |
| `GMAIL_OAUTH_ACCOUNT_MISMATCH` | Google returned a different account than the selected Skarbiec identity | Authorize the exact selected address |
| `GMAIL_AUTHORIZATION_EXPIRED` | Google revoked or rejected the saved refresh token | Reconnect the same Gmail profile; local messages remain readable |
| `MAILBOX_PROFILE_INVALID` | Required host/address/security setting is absent or contradictory | Supply non-secret settings or update the bundle |
| `IMAP_AUTHENTICATION_FAILED` | Provider refused the current credential | Rotate or correct the Skarbiec item, then retry sync |
| `IMAP_UNAVAILABLE` | TLS, DNS, timeout, or provider failure | Inspect mailbox status and retry; other mailboxes continue |
| `SMTP_REJECTED` | Provider rejected the reply before acceptance | Correct the provider/account condition and use a new idempotency key |
| `REPLY_UNCERTAIN` | Process stopped after send began without terminal evidence | Inspect provider Sent mail; no automatic resend occurs |

## Reset and removal

`mailbox remove` deletes the local mailbox, its local messages, and reply records after explicit confirmation. It never deletes the Skarbiec item or provider mailbox. To reset the whole local product, stop Skrzynka and remove the configured database plus SQLite companion files; this is destructive to local normalized mail and reply evidence only.
