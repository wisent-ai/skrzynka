<p align="center">
  <img src="assets/readme-banner.svg" alt="Skrzynka by Wisent — many mailboxes in one reply queue" width="100%">
</p>

<!-- wisent-readme-signals:start -->
[![Source](https://img.shields.io/badge/GitHub-Source-181717?logo=github)](https://github.com/wisent-ai/skrzynka) [![Issues](https://img.shields.io/badge/GitHub-Issues-181717?logo=github)](https://github.com/wisent-ai/skrzynka/issues) [![Wisent](https://img.shields.io/badge/Wisent-Website-0B0B0B)](https://wisent.com) [![Discord](https://img.shields.io/badge/Discord-Join-5865F2?logo=discord&logoColor=white)](https://discord.gg/qRjpkthq54) [![LinkedIn](https://img.shields.io/badge/LinkedIn-Follow-0A66C2?logo=linkedin&logoColor=white)](https://www.linkedin.com/company/wisent-ai/) [![X](https://img.shields.io/badge/X-Follow-000000?logo=x&logoColor=white)](https://x.com/wisentai) [![Enterprise](https://img.shields.io/badge/Enterprise-Book%20a%20call-0B0B0B?logo=calendly)](https://calendly.com/lbartoszcze)
<!-- wisent-readme-signals:end -->

<p align="center">
  <a href="https://github.com/wisent-ai/skrzynka"><img alt="Source" src="https://img.shields.io/badge/GitHub-Source-181717?logo=github"></a>
  <a href="https://skrzynka.wisent.com/docs/api"><img alt="Local API" src="https://img.shields.io/badge/API-loopback%20JSON-0f766e"></a>
  <a href="https://www.rust-lang.org"><img alt="Rust" src="https://img.shields.io/badge/Rust-1.82%2B-000000?logo=rust"></a>
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/License-Apache--2.0-blue"></a>
  <a href="https://discord.gg/qRjpkthq54"><img alt="Wisent Discord" src="https://img.shields.io/badge/Discord-Join-5865F2?logo=discord&logoColor=white"></a>
</p>

# Skrzynka

Skrzynka is a self-hosted email operations service for people and local tools that need to receive mail from multiple IMAP mailboxes, send replies through the mailbox that received each message, and originate new mail from any of those mailboxes. Mailbox credentials stay in [Skarbiec](https://github.com/wisent-ai/skarbiec): Skrzynka stores exact item references and non-secret connection settings, resolves passwords or OAuth authorizations only while opening IMAP or SMTP, and never writes resolved secrets to its database or logs.

The observable result is one local inbox with mailbox identity preserved on every inbound message and provider-facing send. The service exposes the same state through a CLI and an authenticated loopback-only JSON API used by `skrzynka-desktop`.

## What works now

- Connect one personal Gmail account or one Workspace user with an app-specific password and no administrator or OAuth client: `skrzynka account password --provider gmail --email user@example.com` reads the secret only from stdin, proves it with Gmail IMAP before saving anything, writes a dedicated bundle to Skarbiec, and declares it a mailbox there. `POST /v1/gmail/app-password` provides API parity by accepting an existing `skarbiec_item_id`, never the secret.
- Connect Google identities discovered in Skarbiec through Gmail OAuth; Skrzynka configures Gmail, stores the durable authorization back in Skarbiec, and performs IMAP/SMTP authentication with XOAUTH2.
- Connect Google Workspace mailboxes through domain-wide delegation with no consent screen: `skrzynka account delegate --provider gmail --email user@domain` (or `POST /v1/gmail/delegate`) mints XOAUTH2 tokens from the service-account key in the Skarbiec item playing the role `skrzynka-google-service-account`, after a one-time client-ID grant in the Workspace admin console. Skrzynka never performs that grant: it exists only in the admin console, so a missing grant is reported as `GOOGLE_DELEGATION_NOT_GRANTED` naming the three values an administrator needs.
- Ask which of those three paths an account can actually use, and get an answer measured against Google rather than read off the vault: `skrzynka account connection --provider gmail --email user@example.com` (or `GET /v1/gmail/connection?email=`) performs a real IMAP login with the stored password, hands Google the authorization URL a real flow would hand it and reads the code Google returns, and mints a real delegated token for a Workspace address. Each path is `usable`, `refused` or `unproven`, with the refusal code, the provider's own words, the non-secret facts observed, and the exact next step. A consumer `@gmail.com` address is refused for delegation as `GOOGLE_DELEGATION_NOT_APPLICABLE` without calling Google, because no administrator can grant it. `skrzynka account` takes `--provider`, whose one value is `gmail`; a call without it exits 2.
- Skarbiec owns the mailbox list. A Skarbiec `login` or `bundle` item is a
  mailbox exactly when it carries the tag `skrzynka:mailbox`; Skrzynka keeps
  only the mail it imported and where synchronization stands. Tag an item in
  Skarbiec, or run `skrzynka mailbox declare --skarbiec-item <ID>`, and the
  next `mailbox list` or `sync` reads it. CLI, API, and desktop use the same
  credential resolver, IMAP normalizer, and atomic database commit.
  `mailbox list --state failing|undeclared|unsynchronized` (`GET /v1/mailboxes?state=`, Desktop's Mailboxes facets) lists only the
  mailboxes in one state, and `--csv <PATH>` (`GET /v1/mailboxes/csv`, Desktop's **Table of mailboxes**) writes them as a CSV
  inventory without any credential; see `docs/skarbiec/mailbox-states.md`.
- Read password-backed `login` items with explicit non-secret server settings
  or complete `bundle` profiles.
- An omitted `auth_method` identifies a password-backed item. When present,
  it must be `password`, `oauth2` or `oauth2_service_account`; blank, non-text
  and unknown declarations fail with `SKARBIEC_ITEM_INVALID` before provider
  authentication. Profile import and credential use enforce the same choice.
- Poll IMAP over TLS, normalize text messages, and deduplicate them by mailbox
  and IMAP UID. `skrzynka message watch [--mailbox <ID>] [--after <NUMBER>]` (or `GET /v1/messages/watch` as server-sent events, or Skrzynka Desktop's live **Inbox**) hands on each message as an import stores it, woken by the fleet database and resumable from the last `stored_order` without missing or repeating one. See `docs/inbound/watch.md`.
- List mailboxes and messages without exposing credentials. `skrzynka message list … --csv <PATH>` (`GET /v1/messages/csv`, Desktop's **Export as a table (.csv)…** in the inbox filter bar) writes the messages a listing keeps, by its filters, view, order and page, as a CSV table without their text but its snippet to a new file, for a spreadsheet or a report. See `docs/messages/filters.md`.
- Reply through the originating mailbox over SMTP with required TLS and preserved thread headers. `skrzynka message replies <ID>` (or `GET /v1/messages/:id/replies`) reads every reply attempt made to a message with its delivery state; without an ID, `message replies [--mailbox <ID>] [--status <STATUS>] [--since <WHEN>] [--until <WHEN>] [--contains <TEXT>] [--attachment <TEXT>] [--oldest-first]` (`GET /v1/replies`, Desktop's **Replies sent** with **Only since**, **Only before**, **Oldest first**, **Text contains** and **File name contains**) lists every reply sent, whichever message it answered, `--csv <PATH>` (`GET /v1/replies/csv`, Desktop's **Export as a table (.csv)…**) writes them as a CSV table without their text to a new file, and `--resend-failed --idempotency-key <KEY>` (`POST /v1/replies/resend`, Desktop's **Send the failed ones again…**) sends every listed reply that ended `failed` again, never to a message answered since. See `docs/messages/reads.md`.
- Originate mail from a connected mailbox with `skrzynka message send --mailbox <id-or-address> --to <address> --subject <text> --body-file <path> --idempotency-key <key>` (or `POST /v1/mailboxes/:id/outbound`). An outbound message carries its own recipients, cc and subject, walks the same delivery states as a reply, and is claimed by its idempotency key, which every send and reply must state (a command without one is refused with `IDEMPOTENCY_KEY_REQUIRED`), before anything reaches the provider, so a repeated call returns the first attempt instead of sending twice; `--at <INSTANT>` (Desktop's **Send later**) keeps it as a draft a running service sends then. `skrzynka message outbound` reads what went out, `--to <TEXT>`, `--contains <TEXT>`, `--attachment <TEXT>` (a file name it carried), `--status <STATUS>`, `--since`/`--until <WHEN>` (when it was claimed), `--bounced` (a stored bounce report returned it) and `--answered`/`--unanswered` (stored mail answers it, or not yet) find some of it and `--oldest-first` turns the order round (`GET /v1/outbound?to=&contains=&attachment=&status=&since=&until=&bounced=&answered=&oldest_first=`, **Find sent mail** in Skrzynka Desktop's Outbound inspector, with **Oldest first**), `--csv <PATH>` (`GET /v1/outbound/csv`, Desktop's **Export as a table (.csv)…**) writes those messages as a CSV table without their bodies to a new file, `--resend-failed --idempotency-key <KEY>` (`POST /v1/outbound/resend`, Desktop's **Send the failed ones again…**) originates every listed message whose send ended `failed` again as written, each claimed by `<KEY>-<outbound id>`, and `skrzynka message outbound --id <ID>` (or `GET /v1/outbound/:id`) reads one of those messages. See `docs/messages/reads.md`.
- Send blind copies: `--bcc <ADDRESS>` on `message send`, `message forward` and `message draft save|update` (or `bcc` beside `to` and `cc` over HTTP, or **Bcc** on Desktop's Compose, Forward and draft editor) delivers to addresses no header names: they are SMTP envelope recipients only, recorded on the outbound row as `bcc`. See `docs/writing/bcc.md`.
- Attach files to originated mail and to replies: `skrzynka message send … --attach application/pdf=./quote.pdf` and `skrzynka message reply <ID> … --attach TYPE=PATH` (repeatable; the type is stated, never guessed from the name), `attachments: [{filename, content_type, content_base64}]` on `POST /v1/mailboxes/:id/outbound` and `POST /v1/messages/:id/replies`, or **Attach file…** on Compose and in the Inbox reply composer of Skrzynka Desktop (with the MIME type macOS declares for the file). Every file is checked before the row is claimed (`OUTBOUND_ATTACHMENT_INVALID` names the file and what is wrong), sent as `multipart/mixed` after the text, and recorded on the outbound row or reply attempt by name, type, size and SHA-256; the bytes are not kept. The same idempotency key with different files is `IDEMPOTENCY_KEY_REUSED`.
- Record synchronization, reply, and outbound state in the fleet database `skrzynka`, including actionable mailbox errors, the provider's own SMTP refusal text, and ambiguous-send protection.
- Keep the HTTP surface on loopback; startup refuses a non-loopback bind address.
- Authenticate every `/v1` API request through the shared Wisent identity service and scope durable mailbox, message, reply, and outbound access to the selected organization.
- Read a message's attachments: each import records which MIME parts are files (index, name, type, decoded size) in the message's `attachments` field, and `skrzynka message attachment list <ID>` / `skrzynka message attachment save <ID> --index N --output PATH` (`--all --output DIRECTORY` for every one) (or `GET /v1/messages/:id/attachments` and `GET /v1/messages/:id/attachments/:index`, or Skrzynka Desktop's inspector) read the bytes from the provider on demand with `BODY.PEEK[]`; Skrzynka stores no attachment bytes. A message imported before attachments were recorded has `attachments: null` until it is listed once. `message attachment find [--name] [--type] [--over] [--sender] [--since] [--until] [--tag] [--largest-first]` (`GET /v1/attachments`, Desktop's **Files**) lists every recorded file across the stored mail, newest first or largest first, with the message carrying it, such as the invoices received in the last 30 days (`--name invoice --since -30d`) or the files of mail tagged `contracts` (`--tag contracts`), `--csv <PATH>` (`GET /v1/attachments/csv`, Desktop's **Export as a table (.csv)…**) writes them as a CSV table to a new file, and `--save <DIRECTORY>` (Desktop's **Save every file listed to a new folder…**) reads every one from its provider into a new directory as `<message id>-<index>-<name>`. See `docs/messages/attachments.md`.
- Change a message at its provider: `skrzynka message mark <ID> --as seen|unseen|flagged|unflagged` (flagged is Gmail's star), `skrzynka message move <ID> --folder <NAME>` (or `--to archive|trash|junk|all`, the folder the provider declares for that use) and `skrzynka message delete <ID> --confirm` (or `POST /v1/messages/:id/mark`, `/move` and `/delete`, or Skrzynka Desktop's inspector) read the provider's copy by UID, refuse with `MESSAGE_SOURCE_CHANGED` when it is no longer the stored message, act over IMAP and read the provider's state again to observe the effect. Every request is recorded before the provider is asked and ends `done`, `failed` or `uncertain`; `skrzynka message actions <ID>` (`GET /v1/messages/:id/actions`) reads the record. Skrzynka keeps its stored copy of a moved or deleted message. `skrzynka mailbox folders <MAILBOX_ID>` (`GET /v1/mailboxes/:id/folders`, Desktop's **Folders** menu beside **Move**) reads the provider's folders now with IMAP `LIST`, each with whether it holds messages and what the provider says it is for (RFC 6154 `sent`, `trash`, `junk`…); `name` is what `--folder` takes. See `docs/messages/provider-actions.md`.
- Act on many messages at once: `skrzynka message mark --view <NAME> --as seen` (or any other state) and `message move --view <NAME> --folder <FOLDER>` or `--to <USE>` (or `POST /v1/views/:name/mark|move`, or **Mark view**, **Move view** and **Move view to** in Skrzynka Desktop's filter bar) ask the provider to change every message a saved view lists, and `--thread <ID>` in place of `--view` (`POST /v1/messages/:id/thread/mark|move`, Desktop's **At the provider** in the Conversation section) every message of one conversation, such as to archive it; each is its own recorded action, and they print which changed and which were refused and why. See `docs/messages/provider-actions.md`.
- Leave mailing lists: each import records a message's `List-Id`, `List-Unsubscribe` and `List-Unsubscribe-Post` headers in its `mailing_list` field. `skrzynka message subscriptions [--mailbox <ID>] [--subscribed] [--contains <TEXT>] [--since <WHEN>] [--until <WHEN>] [--leavable]` (or `GET /v1/subscriptions`) groups the stored mail by list, most messages first, with each list's ways out and its last request to leave it, `--subscribed` keeping the lists not left yet, `--contains` those whose List-Id or sender contains a text, `--since`/`--until` those whose newest message arrived in a window, such as the lists gone quiet (`--until -90d`, Desktop's **Last message**), and `--leavable` those Skrzynka can leave itself (Desktop's **Can leave from here only**); `--csv <PATH>` (`GET /v1/subscriptions/csv`, Desktop's **Export as a table (.csv)…**) writes them as a CSV table to a new file, and `--leave --idempotency-key <KEY>` (`POST /v1/subscriptions/leave`, Desktop's **Leave every list listed…**) asks every list a narrowed listing keeps to stop sending at once, such as every list quiet for 90 days. `skrzynka message unsubscribe <ID> --idempotency-key <KEY>` (or `POST /v1/messages/:id/unsubscribe`, or **Unsubscribe from …** in Skrzynka Desktop) asks the list that message came from to stop sending, and `--list <LIST_ID> [--mailbox <ID>]` in place of `<ID>` (`POST /v1/subscriptions/unsubscribe`, Desktop's **Unsubscribe** beside a list) asks the list a List-Id names through its newest message: an RFC 8058 one-click POST when the list offers one, otherwise mail to its `mailto:` address from the receiving mailbox; a list that names only web pages is refused with `UNSUBSCRIBE_NEEDS_BROWSER`, because Skrzynka opens no browser. See `docs/lists/unsubscribe.md`.
- Stop seeing a sender: `skrzynka message block <ADDRESS or @DOMAIN>` (or `POST /v1/blocked`, or **Block** beside a contact in Skrzynka Desktop) leaves its mail out of `message list` until `message unblock`; `--include-blocked` lists it anyway and `message blocked` lists the entries, `--count` with how many stored messages each hides and `--contains <TEXT>` only those holding a text; `message blocked --import <FILE>` (`POST /v1/blocked/import`, Desktop's **Import blocked senders…**) blocks a whole list, one entry per line, only when every line reads, `--export <FILE>` (`GET /v1/blocked/export`, Desktop's **Export blocked senders…**) writes the list in that same form, and `--csv <FILE>` (`GET /v1/blocked/csv`, Desktop's **Blocked senders as a table (.csv)…**) as a table of each entry and the mail it hides. Nothing is deleted and the provider is not told. See `docs/lists/blocked.md`.
- Tag mail as it arrives: `skrzynka message rule add --tag <TAG> [--sender <TEXT>] [--list <TEXT>] [--contains <TEXT>] [--recipient <TEXT>] [--attachment <TEXT>] [--mailbox <ID>]` (or `POST /v1/tag-rules`, or **Tag rules** in Skrzynka Desktop's inbox inspector) makes every import give matching messages the tag, of every mailbox or only the one `--mailbox` names; `--recipient` matches the To or Cc header, so mail to one alias gets its own tag, and `--attachment` a file name the message carries, so invoices get theirs. `message rule matches <ID>` (`GET /v1/tag-rules/:id/matches`, Desktop's **Matches**) lists the stored mail a rule matches before anything is tagged, `message rule apply [--rule <ID>]` tags the mail already stored by every rule or one (Desktop's **Apply** beside a rule), `rule update <ID>` (`PUT /v1/tag-rules/:id`, Desktop's **Edit**) replaces a rule's tag and conditions keeping its ID, `rule copy <ID> --tag <TAG> [--mailbox <ID>]` (`POST /v1/tag-rules/:id/copy`, Desktop's **Copy**) makes a new rule with a rule's conditions and another tag, `rule pause|resume <ID>` (`/v1/tag-rules/:id/pause`, Desktop's **Pause** and **Resume**) stops a rule tagging without losing it and lets it tag again, `rule list [--contains <TEXT>] [--paused|--active] [--mailbox <ID>]` (Desktop's **Tag or condition contains**, **Paused only** and **Tagging mail of**) lists them, `rule show <ID>` (`GET /v1/tag-rules/:id`) reads one, and `rule remove` removes one; `rule export --output <FILE>` with the list's flags (`GET /v1/tag-rules/export`, Desktop's **Export rules…**) writes them to one JSON file naming each mailbox by address and `rule import --input <FILE>` (`POST /v1/tag-rules/import`, Desktop's **Import rules…**) adds every rule of such a file on another installation, all checked before any is added. See `docs/lists/rules.md`.
- Reply to everyone, quoting: `skrzynka message reply <ID> --all …` (or `reply_all: true` on `POST /v1/messages/:id/replies`, or **Reply to all** in Skrzynka Desktop's reply composer) also copies every address of the message's To and Cc headers, the mailbox's own and the reply target aside; each import records a message's Cc in its `cc` field for this. `--cc` and `--bcc` copy more addresses, the blind ones named in no header (`cc`/`bcc`, Desktop's **Also copy** and **Blind copy**). `--quote` (`quote: true`, Desktop's **Quote the message below the reply**) puts the message's text below the reply, each line after `> `. `message reply --view <NAME> --template <NAME> …` (`POST /v1/views/:name/replies`, Desktop's **Answer or forward a view**) answers every message a saved view lists, such as every application at once, each claimed by `<KEY>-<message id>` and the template filled for each sender. See `docs/writing/reply-all.md`.
- Put a message aside until later: `skrzynka message snooze <ID> --until <INSTANT>` or `--until +3d` (a span from now in minutes, hours, days or weeks; or `POST /v1/messages/:id/snooze`, or **Snooze** and **Snooze for** in Skrzynka Desktop's message inspector) leaves it out of `message list` until the instant passes, then lists it again; `message wake <ID>` shows it now, `--include-snoozed` lists it anyway and `--snoozed` lists only what is snoozed; `snooze --view <NAME>` (`POST /v1/views/:name/snooze`, Desktop's **Snooze view**) puts every message of a saved view aside at once and `wake --view <NAME>` (`DELETE /v1/views/:name/snooze`, Desktop's **Wake view**) shows them all again, and `--thread <ID>` (`/v1/messages/:id/thread/snooze`, Desktop's **Snooze conversation for** and **Wake conversation**) does the same for one conversation. Nothing changes at the provider. See `docs/lists/snooze.md`.
- Keep notes on mail: `skrzynka message note <ID> --file <PATH>` (or `PUT /v1/messages/:id/note`, or **Note** in Skrzynka Desktop's message inspector) keeps your own text on a stored message in its `note` field; `--append` adds the file's text on a line of its own (Desktop's **Add to note**), `--clear` removes it, and `message list --noted` lists the noted mail. Notes live only in Skrzynka. See `docs/lists/notes.md`.
- Back a mailbox up and put it back: `skrzynka message export --mailbox <ID or address> --output <DIRECTORY>` (or **Export mailbox…** in Skrzynka Desktop's mailbox inspector) creates the directory and saves every stored message there as its original `<id>.eml`, read from the provider, with a `manifest.json` of each file's size and SHA-256 and of every message that could not be read and why; `--view <NAME>` saves only the messages a saved view lists, `--thread <MESSAGE_ID>` one conversation (Desktop's **Export conversation…**), `--since <WHEN>` what arrived since an instant (Desktop's **Only since**), `--until <WHEN>` what arrived before one (Desktop's **Only before**) and `--tag <TAG>` the messages carrying a tag (Desktop's **Only messages tagged**), and `--mbox` (Desktop's **Also one mailbox.mbox for a mail program**) also writes them all into one mboxrd `mailbox.mbox` that Apple Mail or Thunderbird imports. `skrzynka message verify --input <DIRECTORY>` (Desktop's **Check an export…**) checks every file of an export, the mbox included, against its manifest without contacting a provider and ends `EXPORT_DAMAGED` naming each that changed or is gone. `skrzynka message restore --mailbox <ID or address> --input <DIRECTORY> --folder <FOLDER>` (or `POST /v1/mailboxes/:id/append` per message, or **Restore an export…** in Desktop) checks each file against the manifest and appends it to the provider folder with IMAP `APPEND`, skipping a message the folder already holds by its Message-ID. See `docs/backup/export.md`, `docs/backup/verify.md` and `docs/backup/restore.md`.
- Read how a mailbox's sends ended: `skrzynka mailbox delivery <ID>` (or `GET /v1/mailboxes/:id/delivery`, or **Delivery** in Skrzynka Desktop's mailbox inspector) counts every recorded reply, originated message and unsubscribe request by status (`sent`, `failed`, `uncertain`, in flight) and each refusal by its code, and lists every imported bounce report (RFC 3464 delivery status notification) whose returned Message-ID names one of those sends, with each recipient's action, status and diagnostic as the reporting server wrote them; each kind's `bounced` counts the sends those reports returned, to set beside `sent`, `days` counts the same per UTC day, oldest first, to read a warm-up against, and `domains` counts the originated mail's recipients per domain, accepted, failed and returned, to see which receiving provider turns it away (Desktop's **By recipient domain**); `--since`/`--until` count one window only, and `--days-csv <PATH>` or `--domains-csv <PATH>` (`GET /v1/mailboxes/:id/delivery/days|domains/csv`, Desktop's **Days as a table (.csv)…** and **Domains as a table (.csv)…**) write those days or domains as a CSV table to a new file. `sent` means the provider's server accepted the send. See `docs/delivery/report.md`.
- Stop sending to addresses that bounced: `skrzynka message bounced [--since <WHEN>] [--until <WHEN>] [--contains <TEXT>]` (or `GET /v1/bounced`, or **Addresses that bounced** in Skrzynka Desktop's Outbound inspector, with **Reported since**, **Reported before** and **Address contains**) lists every address a stored bounce report says delivery to failed for good, or those reported lately, or long ago, or those of one domain, with how many reports said so and what the newest said; `--csv <PATH>` (`GET /v1/bounced/csv`, Desktop's **Export as a table (.csv)…**) writes them as a CSV table to a new file for a CRM or mailing tool. `message send`, `message forward` and `message draft send` refuse a new send to one of them with `OUTBOUND_RECIPIENT_BOUNCED` before anything is claimed, so mail that cannot arrive does not count against the mailbox; `--allow-bounced` (`allow_bounced: true`, Desktop's **Send even to addresses that bounced**) sends anyway, and `message bounced --clear <ADDRESS>` (`DELETE /v1/bounced/:address`, Desktop's **Works again**) takes an address that works again off the list until a new report, `--clear-listed --contains @<DOMAIN>` (`DELETE /v1/bounced?contains=`, Desktop's **All listed work again…**) every address a narrowed listing keeps. A scheduled draft is always held to the reports. See `docs/delivery/report.md`.
- Never write to someone who asked you not to: `skrzynka message suppressed --add <ADDRESS or @DOMAIN> [--reason <TEXT>]` (or `POST /v1/suppressed`, or **Do not send to** in Skrzynka Desktop's Outbound inspector) keeps the recipient on a suppression list with why, and `--import <FILE>` (`POST /v1/suppressed/import`, Desktop's **Import a list…**) a whole do-not-contact list, one entry per line, kept only when every line reads, while `--export <FILE>` (`GET /v1/suppressed/export`, Desktop's **Export the list…**) writes the list back in that same form and `--csv <FILE>` (`GET /v1/suppressed/csv`, Desktop's **Export as a table (.csv)…**) as a CSV table of entry, reason and instant added; `message suppressed --contains <TEXT>` (Desktop's **Entry or reason contains**) finds entries in it and `--since|--until <WHEN>` (Desktop's **Added**) those added in a window. `message send`, `message forward`, `message draft send` and `draft schedule` naming one are refused with `OUTBOUND_RECIPIENT_SUPPRESSED` before anything is claimed, with no override, and `--remove` (`DELETE /v1/suppressed/:entry`, Desktop's **Remove**) takes it off; `message recipients <ADDRESS>...` (`POST /v1/recipients/check`, Desktop's **Check recipients** on Compose, Forward and the draft editor) says before writing which addresses the list or a bounce report would refuse. Replies are not held to it. See `docs/outreach/suppression.md`.
- Count what a mailbox holds: `skrzynka mailbox summary <ID> [--since <WHEN>] [--until <WHEN>]` (or `GET /v1/mailboxes/:id/summary?since=&until=`, or **Summary** in Skrzynka Desktop's mailbox inspector, with **Only since** and **Only before**) counts its stored messages, or those received in the window, unanswered, snoozed, blocked, muted, tagged, noted, with files and how many bytes those files take (`file_bytes`), bounce reports and failed sender checks, its distinct senders and its first and last receive instants, each the length of the matching `message list` listing, and how many arrived on each UTC day (`days`, Desktop's **Received per day**); `--days-csv <PATH>` (`GET /v1/mailboxes/:id/summary/days/csv`, Desktop's **Received per day as a table (.csv)…**) writes those days as a CSV table to a new file. See `docs/delivery/summary.md`.
- Limit how much a mailbox sends: `skrzynka mailbox limit <ID> --per-day <COUNT>` (or `PUT /v1/mailboxes/:id/limit`, or **Send limit** in Skrzynka Desktop's mailbox inspector) sets how many originated messages, replies and unsubscribe mails the mailbox may send in one UTC day; a send past it is refused with `MAILBOX_SEND_LIMIT_REACHED` before anything is recorded or sent. `skrzynka mailbox limit <ID>` reads the limit and today's count, `--clear` removes it. Warm a new address up with `--grow-by <COUNT> --up-to <COUNT>` (Desktop's **Grow by each day** and **Up to**): the limit starts at `--per-day` and grows by itself every UTC day until the ceiling, so it is ramped up honestly without being raised by hand. See `docs/delivery/limit.md`.
- Sign what a mailbox sends: `skrzynka mailbox signature <ID> --file <PATH>` (or `PUT /v1/mailboxes/:id/signature`, or **Signature** in Skrzynka Desktop's mailbox inspector) sets the text replies, originated messages and forwards carry after an RFC 3676 `-- ` line; `--clear` removes it. Each send records the signature it was claimed with in its row's `signature`, so a later change never alters it. See `docs/delivery/signature.md`. Answer received mail automatically while away: `skrzynka mailbox responder <ID> --file <PATH> --every 7d [--from <WHEN>] [--until <WHEN>]` (or `PUT /v1/mailboxes/:id/auto-reply`, or **Automatic reply** in the mailbox inspector) answers each message received in the window once, at most once per sender within `--every`, as an RFC 3834 auto-reply stating `Auto-Submitted: auto-replied`; mail from the mailbox itself, mail whose headers say a machine sent it (`Auto-Submitted`, `Precedence`, `List-Id`, delivery reports) and senders answered within the span are skipped, and `--decisions` lists every decision with the reply attempt or why none. A running `serve` answers as each import lands; `--answer` does it now. See `docs/replies/auto-reply.md`.
- Read what the receiving provider concluded about a sender: every import records the message's `Authentication-Results` verdicts (SPF, DKIM, DMARC and others, RFC 8601) in its `authentication` field, as the provider wrote them; Skrzynka Desktop shows them as **Sender authentication** in the message inspector. Sending from one declared mailbox to another and reading the received copy's verdicts checks your own domain's setup. A message that is a bounce report carries what it reports in its `bounce` field. See `docs/delivery/authentication.md`.
- Read a whole conversation: `skrzynka message thread <ID>` (or `GET /v1/messages/:id/thread`, or **Conversation** in Skrzynka Desktop's message inspector) lists every stored message of the mailbox that the `Message-ID`, `In-Reply-To` and `References` headers tie to the given one, oldest first. Nothing is guessed from subjects. `message mute <ID>` (`POST /v1/messages/:id/mute`, Desktop's **Mute conversation**) leaves the conversation and every later reply to it out of listings until `message unmute`, and `mute|unmute --view <NAME>` (`/v1/views/:name/mute`, Desktop's **Mute view** and **Unmute view**) does so for every conversation of a saved view; `--include-muted` lists it anyway and `message list --muted` (Desktop's **Muted only**) lists only muted mail. See `docs/conversations/thread.md`.
- Write messages over time: `skrzynka message draft save|update|list|show|delete|send` (or `/v1/drafts`, or **Drafts** in Skrzynka Desktop's inbox inspector) keeps drafts in Skrzynka until `draft send --idempotency-key <KEY>` originates one exactly as `message send` does; a sent draft is removed, a refused one kept. `draft save --template <NAME> --merge <TABLE.csv>` (`POST /v1/drafts/merge`, Desktop's **Merge a table (.csv)…**) is a mail merge: one draft per row of a CSV table, addressed to its `to` column and the template filled with its other columns, every row checked before any draft is kept and nothing sent; `draft schedule --contains <TEXT> --at <INSTANT> --every 5m --idempotency-key <KEY>` (`POST /v1/drafts/schedule`, Desktop's **Schedule the drafts containing it** with **First at** and **Apart**) then schedules them all, spread out so the mailbox sends at a steady pace, all or none, `draft unschedule --contains <TEXT>` (`DELETE /v1/drafts/schedule?contains=`, Desktop's **Unschedule the drafts containing it**) holds them all back again, and `draft delete --contains <TEXT>` (`DELETE /v1/drafts?contains=`, Desktop's **Delete the drafts containing it…**) removes a merge made by mistake; see `docs/outreach/merge.md`. `draft list --scheduled` lists what will be sent, soonest first, `--send-by +1d` (Desktop's **Going out within**) what goes out before a bound, `--contains <TEXT>` the drafts mentioning a text, `--bounced` (Desktop's **To a bounced address**) those addressed to an address that bounced for good, `--suppressed` (Desktop's **To a suppressed recipient**) those addressed to a recipient the suppression list covers, `--with-attachments` (Desktop's **With files**) those carrying a file, and `--since`/`--until <WHEN>` those last changed in a window, such as `--until -7d` for the ones left half-written (Desktop's **Untouched for a week**); `draft list … --csv <PATH>` (`GET /v1/drafts/csv`, Desktop's **Export as a table (.csv)…**) writes the drafts listed as a CSV table without their bodies to a new file. `draft again <OUTBOUND_ID>` (`POST /v1/outbound/:id/draft`, Desktop's **Keep as a new draft**) starts one from a message already originated, and `draft copy <ID> [--mailbox <ID>]` (`POST /v1/drafts/:id/copy`, Desktop's **Copy**) one from another draft, from its mailbox or another. `--attach TYPE=PATH` keeps files with a draft until it is sent (`--no-attachments` removes them; Desktop's draft editor **Attach file…**), listed by name, size and SHA-256 and sent with it. `draft schedule <ID> --at <INSTANT> --idempotency-key <KEY>` has a running `skrzynka serve` send it then, woken by the database at the instant and on every schedule change; a scheduled send that does not end `sent` is unscheduled with why. See `docs/writing/drafts.md`.
- Reuse texts: `skrzynka message template save <NAME> --file <PATH> [--subject <TEXT>]` (or `PUT /v1/templates/:name`, or **Templates** in Skrzynka Desktop's inbox inspector) keeps a text, and the subject a new message from it carries, under a name; `message reply|send|forward … --template <NAME>` and `message draft save|update … --template <NAME>` take it as the body in place of `--body-file`, `send` and `draft` without `--subject` take its subject too, and Desktop's **Templates** menu beside every body puts it there (and its subject into a blank **Subject**). A subject's and text's `{{key}}` placeholders are filled with `--set KEY=VALUE` (`template render`, `POST /v1/templates/:name/render`, Desktop's **Fill**); a missing or unused value is refused. A reply or forward fills `{{sender}}`, `{{sender_name}}` and `{{subject}}` from the message it answers where no `--set` does (`template render --message <ID>` previews it). `template list|show|rename|copy|delete` manage them, `list --contains <TEXT>` (Desktop's **Find**) keeping those whose name, subject or text holds a text, `list --placeholder <KEY>` (Desktop's **Uses placeholder**) those naming a `{{KEY}}`, and `copy` keeping them under a second name; `template export --output <FILE> [--contains <TEXT>] [--placeholder <KEY>]` (`GET /v1/template-document`, Desktop's **Export templates…**) writes them to one JSON file and `template import --input <FILE>` (`POST /v1/template-document`, Desktop's **Import templates…**) keeps every template of such a file on another installation, all checked before any is kept. See `docs/writing/templates.md`.
- Know who owes you an answer: `skrzynka message followup set <OUTBOUND_ID> --by <INSTANT>` or `--by +3d` (or `PUT /v1/outbound/:id/follow-up`, or **Follow-up** and **Follow up in** in Skrzynka Desktop's outbound inspector) expects an answer to a sent message by then; `message send|forward|draft send … --follow-up +3d` (`follow_up`, Desktop's **Expect an answer in** on Compose, Forward and the draft editor) sets it as the message is sent, once it ends `sent`, and `message outbound --contains <TEXT> … --follow-up +5d` (`POST /v1/outbound/follow-ups`, Desktop's **Expect answers to these…** in **Find sent mail**) sets it on every sent message an outbound listing keeps, such as a merge sent from scheduled drafts. `followup list [--state waiting|due|answered] [--mailbox <ID>] [--to <TEXT>] [--contains <TEXT>] [--due-by <WHEN>]` (`GET /v1/follow-ups`, Desktop's **Standing** for `--state`, **Recipient contains** for `--to`, **Subject or body contains** for `--contains` and **Due within** for `--due-by +3d`) says of each, of those standing so, of one mailbox's, of those sent to an address holding a text, of those whose subject or body holds a text, or of those coming due soon, whether it is `waiting`, `due` or `answered`, an answer being a stored message whose In-Reply-To or References names the sent Message-ID, and `--csv <PATH>` (`GET /v1/follow-ups/csv`, Desktop's **Export as a table (.csv)…**) writes them as a CSV table to a new file; `followup show <OUTBOUND_ID>` (`GET /v1/outbound/:id/follow-up`, Desktop's **Follow-up** section) says it of one, and `followup clear --state answered` (`DELETE /v1/follow-ups?state=answered`, Desktop's **Stop expecting these…**) stops expecting every answered one at once, narrowed as `list` narrows. `followup nudge --template <NAME> [--state due] …` (`POST /v1/follow-ups/nudge`, Desktop's **Draft reminders…**) drafts a reminder from a template to every follow-up still owed an answer, `{{subject}}` and `{{due_at}}` filled from the sent message, every recipient checked before any draft is kept. Nothing is sent by a follow-up. See `docs/writing/follow-ups.md`.
- Save a message's original: `skrzynka message source <ID> --output <PATH>` (or `GET /v1/messages/:id/source`, or **Save original (.eml)…** in Skrzynka Desktop's message inspector) reads the whole MIME source from the provider by UID with `BODY.PEEK[]`, only while that UID still holds the stored message, and writes it to a new file that is never overwritten. See `docs/conversations/source.md`.
- Forward a message: `skrzynka message forward <ID> --to <ADDRESS> --body-file <PATH> --idempotency-key <KEY>` (or `POST /v1/messages/:id/forward`, or **Forward…** in Skrzynka Desktop's message inspector) sends it on from the mailbox that received it, as `Fwd: <subject>` with your text and the original attached whole as `message/rfc822`, then any files of your own given with `--attach TYPE=PATH` (Desktop's **Attach file…**). It is originated mail: claimed by its key, counted against the send limit, and listed by `message outbound`. `message forward --view <NAME> …` (`POST /v1/views/:name/forward`, Desktop's **Answer or forward a view**) forwards every message a saved view lists, such as every invoice to an accountant, each claimed by `<KEY>-<message id>` so nothing goes twice, naming each one refused. See `docs/conversations/forward.md`.
- See who you exchange mail with: `skrzynka message contacts [--mailbox <ID>]` (or `GET /v1/contacts`, or **Contacts** in Skrzynka Desktop's inbox inspector) lists every address the stored mail came from or originated mail went to, with the names it was written with, counts each way, the latest instants and whether it bounced for good, is suppressed or is blocked, derived from the mail on every read; `--contains <TEXT>` (`contains=`, Desktop's **Find**) keeps those whose address or a name contains it, `--bounced` (`bounced=true`, Desktop's **Bounced only**) those that bounced, `--suppressed` (`suppressed=true`, Desktop's **Do not send only**) those the suppression list covers, `--blocked` (`blocked=true`, Desktop's **Blocked only**) those a blocked entry covers, `--since <WHEN>` (Desktop's **Exchanged since**) those exchanged with since then, `--until <WHEN>` (Desktop's **Quiet since**) those last exchanged with before then, `--never-written` and `--never-received` (Desktop's **One way**) those who wrote and were never written to, or the reverse, and `--recent` (`recent=true`, Desktop's **Latest first**) lists the latest exchanged with first; `--vcard <PATH>` (`GET /v1/contacts/vcard`, Desktop's **Export as vCards (.vcf)…**) writes them as vCards for an address book and `--csv <PATH>` (`GET /v1/contacts/csv`, Desktop's **Export as a table (.csv)…**) as a CSV table with every count and mark for a spreadsheet or CRM, `--suppress-listed [--reason <TEXT>]` (`POST /v1/contacts/suppress`, Desktop's **Do not send to these…**) puts every contact a narrowed listing keeps on the suppression list, such as everyone written to who never wrote back, and `--block-listed` (`POST /v1/contacts/block`, Desktop's **Block these…**) blocks them so listings leave their mail out. See `docs/conversations/contacts.md`.
- Label mail your own way: `skrzynka message tag <ID> --add <TAG> --remove <TAG>` (or `POST /v1/messages/:id/tags`, or **Tags** in Skrzynka Desktop's message inspector) keeps tags on stored messages in Skrzynka only, never at the provider; `message tag --view <NAME> …` (`POST /v1/views/:name/tags`, Desktop's **Tag view**) changes every message a saved view lists at once, and `--thread <ID>` (`POST /v1/messages/:id/thread/tags`, Desktop's **Tag conversation**) every message of one conversation. Every message carries its `tags`, `message list --tag <TAG>` (`?tag=`) keeps the tagged ones and `--untagged` (`?untagged=true`, Desktop's **Untagged only**) the ones not sorted yet, and `skrzynka message tags [--mailbox <ID>] [--contains <TEXT>] [--since <WHEN>] [--until <WHEN>]` (`GET /v1/tags`, Desktop's **Tags** panel with **Counted in**, **Counted on mail of** and **Tag contains**) lists every tag in use, of every mailbox or one, those holding a text, or those used on the mail of a window, `--csv <PATH>` (`GET /v1/tag-counts/csv`, Desktop's **Export as a table (.csv)…**) writing them as a CSV table of tag and count; `message tags --rename <TAG> --to <NEW>` renames one on every message, tag rule and saved view, and `--remove <TAG>` takes it off every message. See `docs/conversations/tags.md`.

Skrzynka does **not** currently render remote HTML, expose a remote shared server, or generate reply text. Gmail OAuth is an authentication adapter; message transport remains IMAP and SMTP.

## First local result

### Prerequisites

- macOS or Linux on `x86_64` or `arm64`.
- Rust 1.82 or newer for the development channel.
- `skarbiec` on `PATH`, with a local owner key able to open the selected item.
- A mailbox with IMAP over TLS and SMTP over STARTTLS or implicit TLS enabled.
- For the direct Gmail path, 2-Step Verification and an app-specific password generated by that account's owner; no Workspace administrator or OAuth client is needed. Gmail OAuth instead requires one Google login identity in Skarbiec and the Skarbiec item playing the role `skrzynka-google-oauth-desktop`, containing a Google **Desktop app** OAuth client JSON.

Build the development source and inspect the zero-state guidance:

```sh
cargo build
cargo run -- help
```

For a persistent installation, run `stado registry self` to read this Mac's host name,
then `stado product install skrzynka --surface service --host HOST`. Stado owns
the declared service and its automatic supervision; the declaration comes from
`deploy/service.json` through the canonical Wisent product catalogue.
See [managed service and receiving diagnostics](https://skrzynka.wisent.com/docs/managed-service).

For a foreground development session instead:

```sh
cargo run -- serve --bind ADDRESS:PORT
```

`serve` keeps no polling interval. Each enabled mailbox imports what is
waiting, then holds an IMAP IDLE (RFC 2177) and is read again the moment its
provider announces new mail. A mailbox whose sync or IDLE connection fails
records the error on the mailbox (`last_error_code`, `last_error_message`).
Which mailboxes are watched is decided on the fleet database's own
announcements: every write that creates, removes, enables, disables or
re-points a mailbox, or a successful sync that clears its recorded error,
announces itself on the `skrzynka_mailboxes` channel, whichever process made
it, and the running service takes it up at once; no timer restarts a watcher.
`serve` refuses to start when it cannot subscribe, and when a subscription ends
and cannot be taken again it keeps serving while `GET /healthz` answers 503
with status `degraded` and the cause in `watching_stopped`. `skrzynka sync` and
`POST /mailboxes/:id/sync` read a mailbox now.

IMAP connection, XOAUTH2, INBOX selection, UID search and fetch failures retain
the failed operation, configured host/port and the provider or transport cause.
Fetch errors also identify the UID. XOAUTH2 diagnostics redact the supplied
access token and distinguish transport failures from authorization refusals.
CLI, HTTP and Desktop receive the same service error; worker-task failures name
the mailbox and retain the task failure rather than claiming a provider refusal.
If recording that failure in mailbox state also fails, the returned error keeps
the original code and cause and appends the persistence failure; the watcher
logs both. A failed worker task is recorded through the same path. Inspect the
live refusal rather than treating an older `last_error_message` as current.
`IMAP_FETCH_UID_MISMATCH` means a nonempty FETCH response contained no row for
the requested UID. The refusal names the endpoint, requested UID and observed
UIDs; that response cannot advance the cursor or be imported under a guessed
identity. Unsolicited rows are ignored when the requested message is present.
Search results must be strictly newer than the persisted UID cursor. A mailbox
without a cursor searches `ALL`; one with a cursor searches `UID cursor:*`,
which IMAP answers with the cursor's own message, or the newest one when
nothing is newer, and that message is dropped, so it is never fetched or
counted again.
Each import transaction locks the stored mailbox cursor before comparing or
writing messages. If another service instance has advanced it since this pass
read the mailbox, `MAILBOX_SYNC_CURSOR_CHANGED` reports the expected and stored
UIDs without changing that transaction's data. Sync again from the stored cursor;
earlier committed messages remain available.

Google access-token caching uses only a supplied integer `expires_in`. The
deadline is measured from request start, so transport time cannot extend the
provider's lifetime. An absent lifetime means no caching; a malformed or
unrepresentable lifetime returns `GMAIL_TOKEN_RESPONSE_INVALID`, not a panic or
an invented default. Refresh and delegated grants use the same calculation.
Reuse also requires the current credential identity to match the minting
inputs: refresh token, OAuth client and endpoint, or delegated subject, service
account key and scope. The resolver reads the current vault declaration before
consulting cached tokens. A changed or unreadable declaration cannot reuse a
token minted from the old one; only a length-framed digest, not additional
credential plaintext, is retained as the cache identity.
Transport and JSON decoding refusals distinguish refresh-token from
delegated-token operations and retain the underlying cause; decoding failures
also include the observed HTTP status. Diagnostics do not print token response
bodies, assertions or private keys.

In another shell, declare a mailbox whose complete profile is stored in a
Skarbiec `bundle`:

```sh
cargo run -- mailbox declare --skarbiec-item team-inbox
cargo run -- message list
```

`mailbox declare` reads the item's profile, tags the item `skrzynka:mailbox`
in Skarbiec, resolves the credential only inside Skrzynka, and imports every
INBOX message in ascending UID order. Each message is committed with the UID
cursor past it, the first commit adopting the mailbox, so only one message is
held at a time and a failure part way keeps everything before it; the next
`skrzynka sync` continues from the cursor. Its JSON result reports mailbox
state, imported, unchanged, conflicting, and rejected message counts and
rejection reasons; an equal mailbox UID is unchanged and is never inserted
twice. The item must supply the account address and server profile. There
are no local account-profile overrides.

Tagging the item directly works the same way:

```sh
skarbiec retag team-inbox --tags skrzynka:mailbox
cargo run -- sync
```

Every `mailbox list`, `sync`, and watcher round of `serve` first reads which items
Skarbiec declares. It creates a mailbox for a newly declared item, adopts
profile changes of a declared one, and stops watching a mailbox whose item no
longer carries the tag: that mailbox keeps its mail, is shown with
`enabled: false` and `last_error_code: MAILBOX_NOT_DECLARED`. `sync` reports
what this changed under `reconciliation`: `declared`, `created`, `updated`,
`undeclared`, and `refused` items with their exact refusal code and sentence.
`skrzynka mailbox undeclare <id>` removes the tag, keeping the item's other
tags. `skrzynka mailbox remove <id> --confirm` deletes the local mail of an
undeclared mailbox only; a declared one is refused with
`MAILBOX_STILL_DECLARED`.

`skrzynka account disconnect --provider gmail --email <address>` (or the
authenticated `POST /v1/gmail/disconnect` with `{"email": ...}`) is the inverse
of every Gmail connection path. It revokes an OAuth refresh token at Google
first, then moves the credential item the path wrote to Skarbiec's recoverable
trash, and reports each removed `item`, its `auth_method` and `google_grant`
(`revoked`, `already_invalid` when Google had already revoked or expired it,
or `none` for an app password or delegation, which Google revokes in the
account or the admin console). It is refused with `GMAIL_MAILBOX_DECLARED`
while the address is a declared mailbox (undeclare it first), with
`GMAIL_ACCOUNT_NOT_CONNECTED` when Skarbiec holds no credential for it, and
with `GMAIL_REVOKE_UNAVAILABLE` or `GMAIL_REVOKE_FAILED` when Google does not
confirm the revocation; the credential is then kept.

Every command prints its result as JSON, or as `path: value` lines with
`--text`, and a refusal as one JSON `error` object on stderr. It exits 69 when
the error is `retryable` (the same command can succeed later unchanged), 2 for
a wrong invocation or a missing `--organization`, and 1 for any other refusal.

The equivalent reusable surfaces are **Declare** in Skrzynka Desktop and the
authenticated `POST /v1/mailboxes/declare` and
`POST /v1/mailboxes/:id/undeclare`. The secret is never accepted in argv or
the API, and the result does not return it or provider message bodies.
A changed receiving address or IMAP endpoint is a profile conflict and leaves
the retained mailbox unchanged. Differing normalized data for a retained UID
is an import conflict. Each message commits independently: a later failure
preserves earlier committed messages and their cursor, and the next sync
continues from there. Authentication and source failures before import advance
nothing. Unsupported provider rows are counted by reason rather than silently
disappearing. Declaring nothing leaves an empty usable installation.
See the [executable examples](https://skrzynka.wisent.com/docs/examples) and the
[onboarding contract](https://skrzynka.wisent.com/docs/onboarding).

Gmail has three connection paths: an app-specific password for one account,
OAuth, and Workspace domain-wide delegation. `skrzynka account connection`
measures which of them an account can actually use, and
[Connecting Gmail](docs/gmail/connection.md) walks through each one, its
commands and its refusals.

To read the mail one address received from one sender, such as a sign-up confirmation a provider sent to a generated address, narrow the listing: `skrzynka message list --recipient ADDRESS --sender TEXT` keeps the messages whose To header carries the address whole (without case) and whose From header contains the text; `--cc` (an address the Cc header carries), `--contains`, `--since`/`--until` (an instant, a `YYYY-MM-DD` day, or a span back from now such as `-7d`, which a saved view keeps rolling; Desktop's **Received** menu), `--tag`, `--list`, `--attachment` (a file name), `--with-attachments`, `--files-over <BYTES>` (the mail that takes the room), `--bounces`, `--unanswered` (no reply sent yet) or `--answered` (a reply sent) and `--failed-authentication` (a sender verdict other than `pass`) narrow further, `--oldest-first` turns the newest-first order round, `--limit` and `--offset` page the matches, a blank or unreadable filter is refused with `MESSAGE_FILTER_INVALID`, and nothing matching is `[]`. `--save-view <NAME>` keeps the filters as a view and `--view <NAME>` lists one again (`message view list|show|rename|copy|delete`, `/v1/views`, **Views** in Desktop's filter bar; `copy` keeps a view under a second name to change one without the other, Desktop's **Copy view**); `message view list --count` says how many messages each view lists now and `--contains <TEXT>` (Desktop's **Find view**) keeps the views whose name holds a text; `message view export --output <FILE>` (`GET /v1/view-document`, Desktop's **Export views…**) writes them to one JSON file naming each mailbox by address and `message view import --input <FILE>` (`POST /v1/view-document`, Desktop's **Import views…**) keeps every view of such a file on another installation, all checked before any is kept. The same filters are the query parameters of `GET /v1/messages` and the Inbox filter bar in Skrzynka Desktop; every other `--since`/`--until` takes the same three forms; `docs/messages/filters.md` says what matches.

Send a first message from a connected mailbox, addressing it by the mailbox's
own address rather than its id:

```sh
cargo run -- message send \
  --mailbox team@example.com \
  --to buyer@example.net \
  --subject "Quote request" \
  --body-file ./request.txt --idempotency-key quote-request-buyer
cargo run -- message outbound --mailbox team@example.com
```

`--idempotency-key` is required (`IDEMPOTENCY_KEY_REQUIRED` sends nothing without it): the same command with the same key returns the first attempt. `send` prints the stored outbound row: `sent` with the provider message id,
`failed` with the SMTP refusal the server itself returned, or `uncertain` when
the process lost terminal SMTP evidence. `uncertain` is never resent
automatically; a retry needs a new `--idempotency-key`, which is a deliberate
new mutation.

## Skarbiec mailbox contract

The fields a mailbox item carries, how Skrzynka writes and reads credentials,
its token refusals, and the Google OAuth client and service account it asks
Skarbiec for by role are in the
[Skarbiec mailbox contract](docs/skarbiec/mailbox-contract.md).

## Product boundaries and contracts

- [What Skrzynka is](https://skrzynka.wisent.com/docs) and the [executed quick start](https://skrzynka.wisent.com/docs/quick-start)
- [CLI reference](https://skrzynka.wisent.com/docs/cli) and [loopback JSON API](https://skrzynka.wisent.com/docs/api)
- [Concepts](https://skrzynka.wisent.com/docs/concept-mailbox), [architecture](https://skrzynka.wisent.com/docs/architecture), and [configuration](https://skrzynka.wisent.com/docs/configuration)
- [Executed CLI](https://skrzynka.wisent.com/docs/walkthrough-local-lifecycle) and [HTTP](https://skrzynka.wisent.com/docs/walkthrough-loopback-api) walkthroughs
- [Operations runbook](https://skrzynka.wisent.com/docs/runbook) and [canonical examples](https://skrzynka.wisent.com/docs/examples)
- [Product and ownership](https://skrzynka.wisent.com/docs/product), [core state](https://skrzynka.wisent.com/docs/core), and [integrations](https://skrzynka.wisent.com/docs/integrations)
- [Onboarding](https://skrzynka.wisent.com/docs/onboarding) and [release/recovery](https://skrzynka.wisent.com/docs/release)

### Organization authentication

Every `/v1` request except the Gmail provider callback must carry both `Authorization: Bearer <Supabase JWT>` and `X-Wisent-Organization-ID: <uuid>`. Skrzynka forwards the unchanged bearer token and parsed organization UUID to `authorize_organization(target_org_id)` at the Wisent Identity project that `SUPABASE_URL` and `SUPABASE_ANON_KEY` name (Stado's service catalog sets both for the unit; nothing is compiled in, and without them `serve` and `account authorize`, the two commands that serve the loopback API, refuse with `SUPABASE_URL is not set; the central identity authority has no address`; every other command acts for the `--organization` it is given and does not read them) and builds its request context only from the RPC's verified `user_id`, `organization_id`, and `owner`/`admin`/`member` role. It does not query membership tables itself, accept identity or role from a request payload, or log the bearer token.

All three roles can read organization resources, synchronize mailboxes, send replies, and originate mail: `POST /v1/mailboxes/:id/outbound` requires `member`, which `owner`, `admin`, and `member` all satisfy. Mailbox declaration (`POST /v1/mailboxes/declare`, `POST /v1/mailboxes/:id/undeclare`), local mail removal (`DELETE /v1/mailboxes/:id`), Gmail OAuth, app-password item connection, and delegated mailbox connection require `owner` or `admin`. Missing or invalid bearer authentication returns `401`; a missing or malformed organization header returns `400`; missing membership or an unsupported role returns `403`; unavailable central identity verification returns `503`. Workload and service tokens are not human login credentials and receive no synthetic organization context.

## Operating model

Where Skrzynka keeps its state, how every command reaches the fleet database,
how it runs without Stado or Skarbiec, how it acts for one organization, and
what removing a mailbox destroys are in the
[operating model](docs/operating-model.md).

## Status and support

- **Maturity:** development contract, version `0.2.0`; no stable release channel exists yet.
- **Distribution:** source from this repository. A moving `main` branch is not an immutable release coordinate.
- **Compatibility:** fleet database schema version 5; loopback API version 1 (`status` reports `database`, the fleet database's name, where it reported `database_path`); IMAP4rev1 over TLS and SMTP with STARTTLS or implicit TLS.
- **Defects and proposals:** [GitHub Issues](https://github.com/wisent-ai/skrzynka/issues).
- **Private security reports:** use GitHub's private vulnerability reporting for this repository; do not put credentials or message contents in an issue.
- **Community:** [Wisent Discord](https://discord.gg/qRjpkthq54).
- **License:** [Apache-2.0](LICENSE).
