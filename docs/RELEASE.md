# Release and recovery contract

## Version authority

`Cargo.toml` is the only source of the Skrzynka version. `skrzynka version` reports that value. Versions follow Semantic Versioning with the Product Guidelines `0.x` rule: a breaking change advances the minor slot; additive and corrective changes advance the patch slot until the public contract is deliberately declared stable at `1.0.0`.

Public contract includes CLI commands and machine JSON, `/v1` HTTP resources, the mailbox profile, SQLite schema semantics, loopback-only operation, integration capability declarations, and documented reply idempotency behavior.

## Channels

| Channel | Audience | Coordinate | Guarantee |
|---|---|---|---|
| Development | Contributors and controlled local evaluation | Exact source commit | May change; build locally; no automatic upgrade |
| Stable | Not yet open | None | No stable artifact is currently published |

`main` is a moving development branch, not a release coordinate. A future release must use an immutable GitHub release owned by this repository, attach platform/architecture artifacts, SHA-256 digests, source revision, build timestamp, and provenance, and must not replace existing version bytes.

## Compatibility

- API major version is the `/v1` path segment.
- Database schema is recorded with SQLite `PRAGMA user_version`; this release owns schema 1.
- Same-minor `0.x` builds must read the existing schema or refuse startup with an actionable migration error.
- A client must ignore additive JSON fields and must not assume enum values beyond those documented.
- IMAP support is IMAP4rev1 with direct TLS. SMTP support is required STARTTLS or implicit TLS.

## Upgrade

Stop every Skrzynka process sharing the database, back up the database and its `-wal`/`-shm` companions if present, build or install the exact target release, then start one process. Startup applies only migrations explicitly owned by that release. Credentials require no migration because only Skarbiec references are stored.

## Rollback and recovery

A code rollback is safe while the database remains on a schema understood by the older version. If a release advances the schema incompatibly, restore the pre-upgrade database backup before starting the older binary. Never run two versions against one database during rollback.

Reply attempts found in `sending` after an unclean stop become `uncertain`; Skrzynka does not resend them automatically because SMTP may already have accepted the message. The operator inspects provider Sent mail, then creates a new reply attempt only when another send is appropriate.

A database restore recovers local mailbox references and normalized messages. It cannot recover a revoked Skarbiec item or provider mailbox. Reconnect those authorities independently and run synchronization; IMAP UID deduplication prevents duplicate local rows for the retained mailbox record.

## Release notes

Every published release must state added, changed, corrected, removed, and security-relevant behavior; configuration and schema changes; compatibility; operator actions; rollback limits; and known limitations. The release record names the exact examples from the same source revision.
