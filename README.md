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

Skrzynka is a self-hosted email operations service for people and local tools that need to receive mail from multiple IMAP mailboxes and send replies through the mailbox that received each message. Mailbox credentials stay in [Skarbiec](https://github.com/wisent-ai/skarbiec): Skrzynka stores exact item references and non-secret connection settings, resolves passwords or OAuth authorizations only while opening IMAP or SMTP, and never writes resolved secrets to its database or logs.

The observable result is one local inbox with mailbox identity preserved on every message and reply. The service exposes the same state through a CLI and an authenticated loopback-only JSON API used by `skrzynka-desktop`.

## What works now

- Connect Google identities discovered in Skarbiec through Gmail OAuth; Skrzynka configures Gmail, stores the durable authorization back in Skarbiec, and performs IMAP/SMTP authentication with XOAUTH2.
- Connect Google Workspace mailboxes through domain-wide delegation with no consent screen: `skrzynka gmail delegate --email user@domain` (or `POST /v1/gmail/delegate`) mints XOAUTH2 tokens from the service-account key in the Skarbiec item `skrzynka-google-service-account`, after a one-time client-ID grant in the Workspace admin console. `skrzynka gmail delegation` prints the client ID and grant URL.
- Add any number of other mailboxes by exact Skarbiec item ID.
- Read password-backed `login` items with explicit server settings or complete `bundle` profiles.
- Poll IMAP over TLS, normalize text messages, and deduplicate them by mailbox and IMAP UID.
- List mailboxes and messages without exposing credentials.
- Reply through the originating mailbox over SMTP with required TLS and preserved thread headers.
- Record synchronization and reply state in a local SQLite database, including actionable mailbox errors and ambiguous-send protection.
- Keep the HTTP surface on loopback; startup refuses a non-loopback bind address.
- Authenticate every `/v1` API request through the shared Wisent identity service and scope durable mailbox, message, and reply access to the selected organization.

Skrzynka does **not** currently manage provider-side folders, delete or mark messages, download attachments, render remote HTML, expose a remote shared server, generate reply text, or send automatic replies. Gmail OAuth is an authentication adapter; message transport remains IMAP and SMTP.

## First local result

### Prerequisites

- macOS or Linux on `x86_64` or `arm64`.
- Rust 1.82 or newer for the development channel.
- `skarbiec` on `PATH`, with a local owner key able to open the selected item.
- A mailbox with IMAP over TLS and SMTP over STARTTLS or implicit TLS enabled.
- For Gmail, one Google login identity in Skarbiec and the `skrzynka-google-oauth-desktop` item containing a Google **Desktop app** OAuth client JSON.

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

For a Gmail identity, run the loopback OAuth receiver instead of adding the
password-backed login directly:

```sh
cargo run -- gmail authorize --skarbiec-item kimi-lukasz-google-sso
```

The command prints Google's authorization URL, waits on `127.0.0.1:8790`, and
exits only after the callback has stored a dedicated `skrzynka-gmail-*` OAuth
bundle and created the mailbox. `--bind 127.0.0.1:<port>` selects another
loopback port when `8790` is occupied. A browser on a Stado-selected Weles host
can reach that callback through `stado host forward-local`; no provider
password is used for IMAP.


## Skarbiec mailbox contract

For password-backed providers, Skrzynka persists the exact item selected by the operator. A complete Skarbiec `bundle` uses these fields:

| Field | Required | Meaning |
|---|---:|---|
| `username` | yes | IMAP and SMTP authentication identity |
| `password` | password auth | App password or provider-issued mailbox secret |
| `email` | yes | Address used in the outgoing `From` header |
| `imap_host` | yes | IMAP TLS hostname |
| `imap_port` | no | Defaults to `993` |
| `smtp_host` | yes | SMTP hostname |
| `smtp_port` | no | Defaults to `587` for STARTTLS or `465` for implicit TLS |
| `smtp_security` | no | `starttls` (default) or `tls` |
| `display_name` | no | Human-readable mailbox name |

Non-secret connection values supplied during `mailbox add` take precedence over bundle values. Gmail is different: Skrzynka Desktop lists Google identities from Skarbiec and opens the authorization URL returned by the core; Google redirects the browser directly to the core's loopback callback. The core writes a dedicated `skrzynka-gmail-*` bundle containing the refresh token and automatic Gmail server profile. Passwords, OAuth codes, and provider tokens never cross the desktop API.

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

## Operating model

Skrzynka is an operated local product. Its SQLite database defaults to `~/.local/share/skrzynka/skrzynka.db`; it contains organization-scoped mailbox metadata, normalized message content, and reply status, but no mailbox passwords or Wisent session tokens. The service polls enabled mailboxes every 60 seconds by default. Message bodies are bounded to 2 MiB, each sync imports at most 200 messages per mailbox, and dependency retries are explicit rather than infinite.

Back up the database while the service is stopped. Restoring the database restores local messages and mailbox references, but Skarbiec remains authoritative for credentials and the mail provider remains authoritative for provider-side mail. Removing a mailbox from Skrzynka does not delete its Skarbiec item or provider mailbox.

## Status and support

- **Maturity:** development contract, version `0.2.0`; no stable release channel exists yet.
- **Distribution:** source from this repository. A moving `main` branch is not an immutable release coordinate.
- **Compatibility:** SQLite schema version 2; loopback API version 1; IMAP4rev1 over TLS and SMTP with STARTTLS or implicit TLS.
- **Defects and proposals:** [GitHub Issues](https://github.com/wisent-ai/skrzynka/issues).
- **Private security reports:** use GitHub's private vulnerability reporting for this repository; do not put credentials or message contents in an issue.
- **Community:** [Wisent Discord](https://discord.gg/qRjpkthq54).
- **License:** [Apache-2.0](LICENSE).
