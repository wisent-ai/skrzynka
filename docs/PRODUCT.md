# Product contract

## Promise

Skrzynka gives one local operator a unified, durable view of incoming messages from multiple mailboxes and sends a reply through the same mailbox identity that received the message. The product succeeds only when a provider message is persisted locally or a reply is accepted by the configured SMTP server; accepting an API request is not success.

## Actors and outcomes

| Actor | Starting state | Requested outcome | Observable result |
|---|---|---|---|
| Operator | A mailbox credential exists in Skarbiec | Add that mailbox without copying its password | Mailbox inventory names the Skarbiec item and resolved address but contains no secret |
| Operator | One or more enabled mailboxes exist | Receive current provider messages | New messages appear once in the local inbox with their source mailbox preserved |
| Operator | An inbound message exists | Reply as the receiving mailbox | SMTP accepts the reply and the local reply record becomes `sent` |
| Local desktop client | Skrzynka serves its loopback API | Present and operate the same state | UI state matches API resources and normalized errors |

## Vocabulary

- **Mailbox:** one IMAP receive identity plus its matching SMTP send identity.
- **Skarbiec item:** the exact credential item selected for one mailbox. It is the credential authority.
- **Message:** a normalized inbound email stored locally.
- **Reply attempt:** one idempotency-keyed request to answer a stored message.
- **Synchronization:** a bounded read of provider messages after the mailbox's last persisted IMAP UID.
- **Provider state:** mail and folders owned by the IMAP/SMTP provider; Skrzynka does not become their authority.

## Ownership boundaries

Skarbiec owns credential encryption, recovery, rotation, and revocation. The mail provider owns the mailbox, IMAP UIDs, SMTP acceptance, and provider-side retention. Skrzynka owns mailbox references, normalized local messages, synchronization cursors, reply intent, and local operational evidence. `skrzynka-desktop` owns presentation and onboarding only; it does not resolve secrets or talk to mail providers.

## Current constraints

The first contract is single-operator and loopback-only. Authentication is the local OS user boundary. Mail is plain-text normalized; attachments and remote HTML are not retained. Reply generation is human-controlled. No action in Skrzynka mutates provider-side message flags, folders, or deletion state.

## Product Guidelines adoption

This contract follows the `wisent-ai/product-guidelines` dependency order as read from its `main` branch on 2026-08-11: README, release, onboarding, core, integrations, and examples. Product changes update those artifacts in that order before changing public behavior.
