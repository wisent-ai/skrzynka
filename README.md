<p align="center">
  <img src="assets/readme-banner.svg" alt="Skrzynka by Wisent — many mailboxes in one reply queue" width="100%">
</p>

<p align="center">
  <a href="https://github.com/wisent-ai/skrzynka"><img alt="Source" src="https://img.shields.io/badge/GitHub-Source-181717?logo=github"></a>
  <a href="docs/API.md"><img alt="Local API" src="https://img.shields.io/badge/API-loopback%20JSON-0f766e"></a>
  <a href="https://www.rust-lang.org"><img alt="Rust" src="https://img.shields.io/badge/Rust-1.82%2B-000000?logo=rust"></a>
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/License-Apache--2.0-blue"></a>
  <a href="https://discord.gg/qRjpkthq54"><img alt="Wisent Discord" src="https://img.shields.io/badge/Discord-Join-5865F2?logo=discord&logoColor=white"></a>
</p>

# Skrzynka

Skrzynka is a self-hosted email operations service for people and local tools that need to receive mail from multiple IMAP mailboxes and send replies through the mailbox that received each message. Mailbox credentials stay in [Skarbiec](https://github.com/wisent-ai/skarbiec): Skrzynka stores exact item references and non-secret connection settings, resolves secrets only while opening IMAP or SMTP, and never writes resolved passwords to its database or logs.

The observable result is one local inbox with mailbox identity preserved on every message and reply. The service exposes the same state through a CLI and a loopback-only JSON API used by `skrzynka-desktop`.

## What works now

- Add any number of mailboxes by exact Skarbiec item ID.
- Read `login` items (`username`, `password`) with explicit server settings or `bundle` items that also contain the IMAP/SMTP profile.
- Poll IMAP over TLS, normalize text messages, and deduplicate them by mailbox and IMAP UID.
- List mailboxes and messages without exposing credentials.
- Reply through the originating mailbox over SMTP with required TLS and preserved thread headers.
- Record synchronization and reply state in a local SQLite database, including actionable mailbox errors and ambiguous-send protection.
- Keep the HTTP surface on loopback; startup refuses a non-loopback bind address.

Skrzynka does **not** currently manage provider-side folders, delete or mark messages, download attachments, render remote HTML, expose a shared multi-user server, obtain OAuth tokens, generate reply text, or send automatic replies. Provider web APIs are outside the current contract; IMAP and SMTP are the supported integration boundary.

## First local result

### Prerequisites

- macOS or Linux on `x86_64` or `arm64`.
- Rust 1.82 or newer for the development channel.
- `skarbiec` on `PATH`, with a local owner key able to open the selected item.
- A mailbox with IMAP over TLS and SMTP over STARTTLS or implicit TLS enabled.

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

A successful sync prints a bounded JSON summary with `received`, `mailbox_id`, and `last_uid`; `message list` then shows normalized message metadata and body text from the local database. A `login` item can be added by also supplying `--email`, `--imap-host`, `--smtp-host`, and the applicable ports/security mode. See the executable [examples](examples/README.md) and the [onboarding contract](docs/ONBOARDING.md).

## Skarbiec mailbox contract

Skrzynka persists the exact item ID selected by the operator; it never discovers mailboxes by parsing item names. The preferred item is a Skarbiec `bundle` with these fields:

| Field | Required | Meaning |
|---|---:|---|
| `username` | yes | IMAP and SMTP authentication identity |
| `password` | yes | App password or provider-issued mailbox secret |
| `email` | yes | Address used in the outgoing `From` header |
| `imap_host` | yes | IMAP TLS hostname |
| `imap_port` | no | Defaults to `993` |
| `smtp_host` | yes | SMTP hostname |
| `smtp_port` | no | Defaults to `587` for STARTTLS or `465` for implicit TLS |
| `smtp_security` | no | `starttls` (default) or `tls` |
| `display_name` | no | Human-readable mailbox name |

Non-secret connection values supplied during `mailbox add` take precedence over values in the item. The authentication identity and password always come from Skarbiec and cannot be supplied through Skrzynka's API or command line.

## Product boundaries and contracts

- [Product and ownership](docs/PRODUCT.md)
- [Release, compatibility, and recovery](docs/RELEASE.md)
- [Zero-state and first-success journey](docs/ONBOARDING.md)
- [Core state and failure semantics](docs/CORE.md)
- [Skarbiec, IMAP, and SMTP integrations](docs/INTEGRATIONS.md)
- [Loopback JSON API](docs/API.md)
- [Canonical examples](examples/README.md)

## Operating model

Skrzynka is an operated local product. Its SQLite database defaults to `~/.local/share/skrzynka/skrzynka.db`; it contains mailbox metadata, normalized message content, and reply status, but no mailbox passwords. The service polls enabled mailboxes every 60 seconds by default. Message bodies are bounded to 2 MiB, each sync imports at most 200 messages per mailbox, and dependency retries are explicit rather than infinite.

Back up the database while the service is stopped. Restoring the database restores local messages and mailbox references, but Skarbiec remains authoritative for credentials and the mail provider remains authoritative for provider-side mail. Removing a mailbox from Skrzynka does not delete its Skarbiec item or provider mailbox.

## Status and support

- **Maturity:** development contract, version `0.1.0`; no stable release channel exists yet.
- **Distribution:** source from this repository. A moving `main` branch is not an immutable release coordinate.
- **Compatibility:** SQLite schema version 1; loopback API version 1; IMAP4rev1 over TLS and SMTP with STARTTLS or implicit TLS.
- **Defects and proposals:** [GitHub Issues](https://github.com/wisent-ai/skrzynka/issues).
- **Private security reports:** use GitHub's private vulnerability reporting for this repository; do not put credentials or message contents in an issue.
- **Community:** [Wisent Discord](https://discord.gg/qRjpkthq54).
- **License:** [Apache-2.0](LICENSE).
