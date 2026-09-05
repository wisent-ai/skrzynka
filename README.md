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

- Connect one personal Gmail account or one Workspace user with an app-specific password and no administrator or OAuth client: `skrzynka gmail app-password --email user@gmail.com` reads the secret only from stdin, proves it with Gmail IMAP before saving anything, and writes a dedicated bundle to Skarbiec. Add `--mailbox <id-or-address>` to attach that bundle as the receiving credential of an existing mailbox while preserving its address, display name, SMTP profile, and sending credential. Without `--mailbox`, an address match is reconnected as before; otherwise Skrzynka creates the fixed Gmail mailbox profile. `POST /v1/gmail/app-password` provides API parity by accepting an existing `skarbiec_item_id` and the same optional `mailbox` selector, never the secret.
- Connect Google identities discovered in Skarbiec through Gmail OAuth; Skrzynka configures Gmail, stores the durable authorization back in Skarbiec, and performs IMAP/SMTP authentication with XOAUTH2.
- Connect Google Workspace mailboxes through domain-wide delegation with no consent screen: `skrzynka gmail delegate --email user@domain` (or `POST /v1/gmail/delegate`) mints XOAUTH2 tokens from the service-account key in the Skarbiec item `skrzynka-google-service-account`, after a one-time client-ID grant in the Workspace admin console. `skrzynka gmail delegation` prints the client ID, the scope and the console URL. Skrzynka never performs that grant: it exists only in the admin console, so a missing grant is reported as `GOOGLE_DELEGATION_NOT_GRANTED` naming the three values an administrator needs.
- Add any number of other mailboxes by exact Skarbiec item ID.
- Read password-backed `login` items with explicit server settings or complete `bundle` profiles.
- Poll IMAP over TLS, normalize text messages, and deduplicate them by mailbox and IMAP UID.
- List mailboxes and messages without exposing credentials.
- Reply through the originating mailbox over SMTP with required TLS and preserved thread headers.
- Originate mail from a connected mailbox with `skrzynka message send --mailbox <id-or-address> --to <address> --subject <text> --body-file <path>` (or `POST /v1/mailboxes/:id/outbound`). An outbound message carries its own recipients, cc and subject, walks the same delivery states as a reply, and is claimed by its idempotency key before anything reaches the provider, so a repeated call returns the first attempt instead of sending twice. `skrzynka message outbound` reads what went out.
- Record synchronization, reply, and outbound state in a local SQLite database, including actionable mailbox errors, the provider's own SMTP refusal text, and ambiguous-send protection.
- Keep the HTTP surface on loopback; startup refuses a non-loopback bind address.
- Authenticate every `/v1` API request through the shared Wisent identity service and scope durable mailbox, message, reply, and outbound access to the selected organization.

Skrzynka does **not** currently manage provider-side folders, delete or mark messages, download attachments, render remote HTML, expose a remote shared server, generate reply text, or send automatic replies. Gmail OAuth is an authentication adapter; message transport remains IMAP and SMTP.

## First local result

### Prerequisites

- macOS or Linux on `x86_64` or `arm64`.
- Rust 1.82 or newer for the development channel.
- `skarbiec` on `PATH`, with a local owner key able to open the selected item.
- A mailbox with IMAP over TLS and SMTP over STARTTLS or implicit TLS enabled.
- For the direct Gmail path, 2-Step Verification and an app-specific password generated by that account's owner; no Workspace administrator or OAuth client is needed. Gmail OAuth instead requires one Google login identity in Skarbiec and the `skrzynka-google-oauth-desktop` item containing a Google **Desktop app** OAuth client JSON.

Build the development source and inspect the zero-state guidance:

```sh
cargo build
cargo run -- help
```

Start the local service:

```sh
cargo run -- serve
```

In another shell, add a mailbox whose complete profile is stored in a Skarbiec `bundle`:

```sh
cargo run -- mailbox add --skarbiec-item team-inbox
cargo run -- sync
cargo run -- message list
```

A successful sync prints a bounded JSON summary with `received`, `mailbox_id`, and `last_uid`; `message list` then shows normalized message metadata and body text from the local database. A `login` item can be added by also supplying `--email`, `--imap-host`, `--smtp-host`, and the applicable ports/security mode. See the [executable examples](https://skrzynka.wisent.com/docs/examples) and the [onboarding contract](https://skrzynka.wisent.com/docs/onboarding).

For one Gmail account, generate an app-specific password in that account, then
pass it to Skrzynka only through stdin:

```bash
read -rsp "Gmail app-specific password: " GMAIL_APP_PASSWORD; printf '\n'
printf '%s\n' "$GMAIL_APP_PASSWORD" |
  target/debug/skrzynka gmail app-password --email user@gmail.com
unset GMAIL_APP_PASSWORD
target/debug/skrzynka sync
target/debug/skrzynka message list
```

When the Gmail login address differs from the address recipients know, attach
the receiving credential to the existing mailbox instead of creating another
row:

```bash
printf '%s\n' "$GMAIL_APP_PASSWORD" |
  target/debug/skrzynka gmail app-password \
    --email account@workspace.example \
    --mailbox public-alias@example.com
```

The command first logs in to `imap.gmail.com:993` over TLS. Only after Google
accepts the credential does it write the deterministic
`skrzynka-gmail-app-password-*` bundle to Skarbiec. With `--mailbox`, the
selector accepts that mailbox's id or address; Skrzynka changes only its
receiving Skarbiec item, Gmail IMAP endpoint, and enabled state. Its public
address, display name, SMTP host, port, security, and sending Skarbiec item are
preserved. Without `--mailbox`, an existing case-insensitive address match is
reconnected and otherwise a mailbox using `smtp.gmail.com:587` with STARTTLS is
created. If the database change fails after the bundle is saved, the refusal
names both the saved Skarbiec item and the mailbox that was not created or
updated. This is a one-user, one-app-password path: it needs neither a Workspace
administrator nor any OAuth client. The password never appears in argv,
Skrzynka logs, its database, or its loopback API.

The OAuth alternative remains:

```sh
cargo run -- gmail authorize --skarbiec-item kimi-lukasz-google-sso
```

It prints Google's authorization URL, waits on `127.0.0.1:8790`, and exits
after the callback stores a dedicated `skrzynka-gmail-*` OAuth bundle and
creates the mailbox.

Measured on 2026-09-03 against the client currently stored in
`skrzynka-google-oauth-desktop`: Google refuses that authorization with
`redirect_uri_mismatch` for `http://127.0.0.1:8790/v1/gmail/oauth/callback`
and for the bare `http://127.0.0.1:8790`, `http://127.0.0.1/` and
`http://localhost:8790/` variants — the client has no loopback redirect
registered at all, so no consent screen is reachable and this command cannot
complete until the replacement Desktop client described below is created.
Until that client is repaired, use an app-specific password for one account or domain-wide delegation for an administrator-managed Workspace domain.

Send a first message from a connected mailbox, addressing it by the mailbox's
own address rather than its id:

```sh
cargo run -- message send \
  --mailbox team@example.com \
  --to buyer@example.net \
  --subject "Quote request" \
  --body-file ./request.txt
cargo run -- message outbound --mailbox team@example.com
```

`send` prints the stored outbound row: `sent` with the provider message id,
`failed` with the SMTP refusal the server itself returned, or `uncertain` when
the process lost terminal SMTP evidence. `uncertain` is never resent
automatically; a retry needs a new `--idempotency-key`, which is a deliberate
new mutation.



## Skarbiec mailbox contract

For password-backed providers, Skrzynka persists the exact item selected by the operator. A complete Skarbiec `bundle` uses these fields:

| Field | Required | Meaning |
|---|---:|---|
| `username` | yes | Authentication identity for the protocol using this item |
| `password` | password auth | App password or provider-issued mailbox secret |
| `email` | yes | Default address used in the outgoing `From` header |
| `imap_host` | yes | IMAP TLS hostname |
| `imap_port` | no | Defaults to `993` |
| `smtp_host` | yes | SMTP hostname |
| `smtp_port` | no | Defaults to `587` for STARTTLS or `465` for implicit TLS |
| `smtp_security` | no | `starttls` (default) or `tls` |
| `display_name` | no | Human-readable mailbox name |

Non-secret connection values supplied during `mailbox add` take precedence over bundle values. Each mailbox stores a receiving `skarbiec_item_id` and an optional `smtp_skarbiec_item_id`; IMAP always uses the receiving item, while replies and originated messages use the SMTP item when present and otherwise fall back to the receiving item. `gmail app-password` reads the password from stdin, proves it through IMAP, and then writes a canonical `skarbiec.item.v2` bundle with `auth_method: "password"`, the account as both `username` and `email`, and Gmail's fixed IMAP/SMTP profile. A new mailbox uses that whole profile. With `--mailbox`, the CLI attaches it only for receiving and preserves the selected row's external identity and complete sending profile; the API accepts the same selector in its optional `mailbox` field. Skarbiec remains the only credential store and no secret crosses the desktop API. Gmail OAuth instead stores the refresh token in a dedicated Skarbiec bundle, references Skrzynka's fixed Desktop OAuth client item, and resolves a short-lived access token only at connection time. Delegated Gmail stores no mailbox secret at all: its bundle references `skrzynka-google-service-account`, and Skrzynka signs a per-user JWT assertion to mint the short-lived token.

The OAuth client item has ID `skrzynka-google-oauth-desktop`, kind `stado-secret`, and one `value` field of type `oauth_client`; that value is the unmodified JSON downloaded for a Google OAuth client whose application type is **Desktop app**. Skrzynka accepts only the `installed` client shape and Google's canonical authorization and token endpoints.

That client belongs in Wisent's own Google Cloud project `wisent-480400` (project number `1080673333190`), which already holds the OAuth consent brand titled "Wisent" with support email `lukasz.bartoszcze@wisent.ai`. **Desktop app** is not a preference: Google grants installed-app clients the loopback exemption, so any `127.0.0.1` port is accepted without registering each one, and a single Desktop client therefore satisfies Skrzynka's `127.0.0.1:8790` callback and Oko's `--redirect-port` at the same time. A Web-type client has no such exemption and refuses every unregistered redirect URI with `redirect_uri_mismatch` before any consent screen appears. The client id Skrzynka and Oko have been pointed at so far, `903183433368-5nt0jdbqtli8rm39oh2s0limiljap3l9.apps.googleusercontent.com`, is not in that project: the `903183433368` prefix is the project number of `controlai-406621` ("ControlAI"), so the user grant is currently minted through a client owned by an unrelated project. Its registration cannot be repaired with tooling — the only Google Cloud OAuth-client APIs are IAP's, whose clients are locked to IAP usage and expose no redirect-URI field, and which by documentation do not operate on Console-created clients — so the replacement Desktop client must be created once in the Console under `wisent-480400` and its JSON stored as `skrzynka-google-oauth-desktop`.

## Product boundaries and contracts

- [What Skrzynka is](https://skrzynka.wisent.com/docs) and the [executed quick start](https://skrzynka.wisent.com/docs/quick-start)
- [CLI reference](https://skrzynka.wisent.com/docs/cli) and [loopback JSON API](https://skrzynka.wisent.com/docs/api)
- [Concepts](https://skrzynka.wisent.com/docs/concept-mailbox), [architecture](https://skrzynka.wisent.com/docs/architecture), and [configuration](https://skrzynka.wisent.com/docs/configuration)
- [Executed CLI](https://skrzynka.wisent.com/docs/walkthrough-local-lifecycle) and [HTTP](https://skrzynka.wisent.com/docs/walkthrough-loopback-api) walkthroughs
- [Operations runbook](https://skrzynka.wisent.com/docs/runbook) and [canonical examples](https://skrzynka.wisent.com/docs/examples)
- [Product and ownership](https://skrzynka.wisent.com/docs/product), [core state](https://skrzynka.wisent.com/docs/core), and [integrations](https://skrzynka.wisent.com/docs/integrations)
- [Onboarding](https://skrzynka.wisent.com/docs/onboarding) and [release/recovery](https://skrzynka.wisent.com/docs/release)

### Organization authentication

Every `/v1` request except the Gmail provider callback must carry both `Authorization: Bearer <Supabase JWT>` and `X-Wisent-Organization-ID: <uuid>`. Skrzynka forwards the unchanged bearer token and parsed organization UUID to `authorize_organization(target_org_id)` at the canonical Supabase project (`https://alvaewvbyxpgwdpugnxy.supabase.co`) and builds its request context only from the RPC's verified `user_id`, `organization_id`, and `owner`/`admin`/`member` role. It does not query membership tables itself, accept identity or role from a request payload, or log the bearer token.

All three roles can read organization resources, synchronize mailboxes, send replies, and originate mail: `POST /v1/mailboxes/:id/outbound` requires `member`, which `owner`, `admin`, and `member` all satisfy. Mailbox configuration, including Gmail OAuth, app-password item connection, delegated mailbox connection, and mailbox create/update/delete, requires `owner` or `admin`. Missing or invalid bearer authentication returns `401`; a missing or malformed organization header returns `400`; missing membership or an unsupported role returns `403`; unavailable central identity verification returns `503`. Workload and service tokens are not human login credentials and receive no synthetic organization context.

## Operating model

Skrzynka is an operated local product. Its SQLite database defaults to `~/.local/share/skrzynka/skrzynka.db`; it contains organization-scoped mailbox metadata, normalized inbound message content, reply bodies and delivery state, and every originated message's recipients, cc, subject, full plain-text body, delivery status, provider message id, and refusal, but no mailbox passwords or Wisent session tokens. The service polls enabled mailboxes every 60 seconds by default. Message bodies are bounded to 2 MiB, each sync imports at most 200 messages per mailbox, and dependency retries are explicit rather than infinite.

Back up the database while the service is stopped. Restoring the database restores mailbox references, normalized messages, reply attempts, and outbound messages, but Skarbiec remains authoritative for credentials and the mail provider remains authoritative for provider-side mail. Removing a mailbox from Skrzynka deletes that local mailbox and cascades through its messages, reply attempts, and outbound messages, destroying the installation's record that the mailbox originated those messages; it does not delete the Skarbiec item or provider mailbox.

## Status and support

- **Maturity:** development contract, version `0.2.0`; no stable release channel exists yet.
- **Distribution:** source from this repository. A moving `main` branch is not an immutable release coordinate.
- **Compatibility:** SQLite schema version 4; loopback API version 1; IMAP4rev1 over TLS and SMTP with STARTTLS or implicit TLS.
- **Defects and proposals:** [GitHub Issues](https://github.com/wisent-ai/skrzynka/issues).
- **Private security reports:** use GitHub's private vulnerability reporting for this repository; do not put credentials or message contents in an issue.
- **Community:** [Wisent Discord](https://discord.gg/qRjpkthq54).
- **License:** [Apache-2.0](LICENSE).
