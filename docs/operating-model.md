# Operating model

Skrzynka is an operated product whose state lives in the fleet database `skrzynka`, which Stado provisions (`stado database create skrzynka --consumer skrzynka`) and names; every host and organization reads the same record. It contains organization-scoped mailbox metadata, normalized inbound message content, reply bodies and delivery state, and every originated message's recipients, cc, subject, full plain-text body, delivery status, provider message id, and refusal, but no mailbox passwords or Wisent session tokens. `skrzynka serve` keeps no polling interval: each enabled mailbox holds an IMAP IDLE and is read the moment its provider announces new mail. Each sync imports every new message whole, committing each with the cursor past it, and dependency retries are explicit rather than infinite.

Every command connects in four steps, and a failure answers `DATABASE_UNREACHABLE` naming the step:

1. `stado database resolve skrzynka --consumer skrzynka --json` names the Skarbiec item holding the address (`skrzynka-database`).
2. `stado service directory connect skarbiec --consumer skrzynka --json` gives the Skarbiec route.
3. `stado credentials get skrzynka-database --field pooler_url` and `--field ca_certificate`, as consumer `skrzynka-database-client` with the bearer in `~/.stado/skrzynka-database-client-skarbiec-token`, give the pooler URL and the provider's root certificate.
4. Skrzynka connects over TLS verified against that certificate and creates any missing table.

Stado and that bearer are found under `SKRZYNKA_FLEET_HOME`, else `HOME`; with neither set a command is refused with `neither SKRZYNKA_FLEET_HOME nor HOME is set`, and `skrzynka onboarding` without `XDG_STATE_HOME` or `HOME` is refused with `neither XDG_STATE_HOME nor HOME is set`; nothing is guessed.

## Without Stado or Skarbiec

Skrzynka runs on a machine that has neither. `SKRZYNKA_DATABASE_URL` (a
`postgres://` or `mysql://` URL) and `SKRZYNKA_DATABASE_CA_FILE` (the PEM bundle
the server is verified against) name the database; the four steps above are
then skipped. `SKRZYNKA_CREDENTIALS_FILE` names an owner-only (`chmod 600`)
JSON file that answers in Skarbiec's place: item id mapped to the same item
Skarbiec stores (`kind`, `fields`, `context`) plus its `tags`. `mailbox declare`
tags items there, Gmail connections write their bundles there, and every
credential is read from it. A file other users can read, or one that is not a
JSON object, is refused as `SKARBIEC_UNAVAILABLE` with the path and the reason.

## Organizations

The CLI acts for one organization, named by `--organization <ID>`; every command except `serve`, `version` and `onboarding` needs it, and a command without it is refused with `ORGANIZATION_REQUIRED` rather than assuming one. A command that names its organization reconciles only that organization's mailboxes against the Skarbiec declarations it reads; only the background poll of `skrzynka serve` reconciles every organization, against the vault of the host it runs on. Rows written before organizations existed carry the stored namespace `legacy-local`.

## Source layout

The source is one module per concern, each a folder of files under 300 lines:
`cli` (arguments and subcommands), `api` (router and handlers), `service`
(the `AppState` methods, grouped by Gmail connection, mailboxes, polling,
messages and outbound mail), `db` (schema and one file per table), `skarbiec`
(the vault transport, items, mailbox resolution and Google tokens), `gmail`
(the OAuth broker and its diagnosis), `onboarding`, `models`, `mail`, `auth`
and `error`.

## Backups and removal

The fleet database is Supabase Postgres; its backups follow that project. Skarbiec remains authoritative for credentials and the mail provider remains authoritative for provider-side mail. Removing a mailbox from Skrzynka deletes that mailbox and cascades through its messages, reply attempts, and outbound messages, destroying the record that the mailbox originated those messages; it does not delete the Skarbiec item or provider mailbox. Mail kept only in a former local `skrzynka.db` is not carried over: declaring the same Skarbiec item again imports the provider's INBOX anew.
