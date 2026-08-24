# Skrzynka examples

These examples cover the development-channel CLI outcomes documented by the root [README](../README.md). They use the installed `skrzynka` public interface, exact Skarbiec item references, isolated database paths, bounded reads, and explicit provider-facing confirmation. The command paths were exercised with the source-built 0.2.0 binary in an isolated database and stand-in Skarbiec; captured results are in the [CLI lifecycle](../docs/walkthrough-local-lifecycle.md) and [HTTP boundary](../docs/walkthrough-loopback-api.md). No real IMAP inbox or SMTP recipient was used.

## Risk labels

- **Local mutation:** writes only the selected local SQLite database.
- **Credentialed read:** resolves one named Skarbiec item and reads IMAP without changing provider message state.
- **Provider-facing mutation:** submits a real SMTP message.
- **Destructive local mutation:** removes local normalized mail and reply evidence; never removes Skarbiec or provider state.

## Coverage

| Actor | Outcome | Interface | Risk | Canonical example | Evidence status |
|---|---|---|---|---|---|
| New operator | Add one mailbox from Skarbiec and receive mail | CLI | Local mutation, credentialed read | [`getting-started/add-and-sync-mailbox.sh`](getting-started/add-and-sync-mailbox.sh) | Add and unavailable-IMAP path captured; provider success requires a controlled mailbox |
| Operator | List/show mailboxes and messages | CLI | Read-only | Getting-started example | Empty list and mailbox show captured |
| Operator | Synchronize one or all enabled mailboxes | CLI | Credentialed read | Getting-started example | Single/all failure shapes captured; provider success requires a controlled mailbox |
| Operator | Reply through the source mailbox | CLI | Provider-facing mutation | [`core/reply-to-message.sh`](core/reply-to-message.sh) | File and missing-message refusals captured; SMTP send requires a controlled recipient |
| Operator | Disable and remove local mailbox state | CLI | Destructive local mutation | [`operations/remove-local-mailbox.sh`](operations/remove-local-mailbox.sh) | Disable, enable, confirmation refusal, and removal captured |
| Operator | Diagnose unavailable Skarbiec, IMAP, or SMTP | CLI/API | No additional side effect | [Runbook](../docs/runbook.md) | Skarbiec/IMAP isolated failures captured; SMTP from source strings |
| Desktop client | Health and protected resources | Loopback API | Matches underlying operation | [`api/use-loopback-api.sh`](api/use-loopback-api.sh) | Health and fail-closed auth captured; authenticated provider path requires a real session |
| Operator | Provider folder mutation, attachments, HTML, automated replies | — | — | Not supported; see [product contract](../docs/PRODUCT.md) | Declared unavailable |
| Operator | Backup, upgrade, rollback, interrupted-send recovery | Documented operation | Local state/recovery | [Release contract](../docs/RELEASE.md) | Procedure contract |

## Shared prerequisites

- Development source version `0.2.0` built as `target/debug/skrzynka`, or `SKRZYNKA_BIN` set to another exact build.
- A supported `skarbiec` executable on `PATH`.
- For provider examples, a non-production mailbox item and an inbox with at least one known message.
- A new state path chosen specifically for the example. Scripts refuse to reuse or overwrite it.

No example accepts or prints a password. Credential rotation and revocation remain Skarbiec operations. Every provider example uses addresses already present in the selected mailbox/message and has no built-in real recipient.

## Selection

Start with `getting-started/add-and-sync-mailbox.sh`. Use the printed mailbox and message UUIDs as inputs to adjacent examples. Keep the state directory only when you intend to continue; otherwise use the cleanup command printed by the script.

The loopback API example exercises the same owned resources for a desktop or automation client. It never binds a public interface.

Return to the [root README](../README.md), [onboarding](../docs/ONBOARDING.md), [core behavior](../docs/CORE.md), or [integration contracts](../docs/INTEGRATIONS.md).
