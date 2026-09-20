use rusqlite::{params, Connection};
use std::process::{Command, Stdio};

use super::fixture::*;

#[test]
fn mailbox_disable_changes_only_enabled_state_and_refuses_unknown_accounts() {
    let fixture = MailboxFixture::new("disable");
    fixture.seed_mailbox_item("team-inbox");
    let mailbox = fixture.add_mailbox("team-inbox");
    let id = MailboxFixture::mailbox_id(&mailbox);

    let disabled = fixture.skrzynka(&["mailbox", "disable", id]);
    fixture.assert_success("disable mailbox", disabled);
    let state = fixture
        .connection()
        .query_row(
            "SELECT enabled, skarbiec_item_id, email, last_uid FROM mailboxes WHERE id=?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .expect("read disabled mailbox state");
    assert_eq!(
        state,
        (0, "team-inbox".into(), "team@example.invalid".into(), 0)
    );

    let unknown = fixture.skrzynka(&["mailbox", "disable", UNKNOWN_MAILBOX]);
    assert_exit_one_with(
        &unknown,
        r#"{"error":{"code":"NOT_FOUND","message":"mailbox was not found","retryable":false}}"#,
    );
}

#[test]
fn mailbox_enable_changes_only_enabled_state_and_refuses_unknown_accounts() {
    let fixture = MailboxFixture::new("enable");
    fixture.seed_mailbox_item("team-inbox");
    let mailbox = fixture.add_mailbox("team-inbox");
    let id = MailboxFixture::mailbox_id(&mailbox);
    fixture.assert_success(
        "prepare disabled mailbox",
        fixture.skrzynka(&["mailbox", "disable", id]),
    );

    let enabled = fixture.skrzynka(&["mailbox", "enable", id]);
    fixture.assert_success("enable mailbox", enabled);
    let state = fixture
        .connection()
        .query_row(
            "SELECT enabled, skarbiec_item_id, email, last_uid FROM mailboxes WHERE id=?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .expect("read enabled mailbox state");
    assert_eq!(
        state,
        (1, "team-inbox".into(), "team@example.invalid".into(), 0)
    );

    let unknown = fixture.skrzynka(&["mailbox", "enable", UNKNOWN_MAILBOX]);
    assert_exit_one_with(
        &unknown,
        r#"{"error":{"code":"NOT_FOUND","message":"mailbox was not found","retryable":false}}"#,
    );
}

#[test]
fn mailbox_remove_requires_confirmation_deletes_local_state_and_preserves_skarbiec() {
    let fixture = MailboxFixture::new("remove");
    fixture.seed_mailbox_item("team-inbox");
    let mailbox = fixture.add_mailbox("team-inbox");
    let id = MailboxFixture::mailbox_id(&mailbox);

    let unconfirmed = fixture.skrzynka(&["mailbox", "remove", id]);
    assert_exit_one_with(
        &unconfirmed,
        r#"{"error":{"code":"CONFIRMATION_REQUIRED","message":"mailbox removal requires --confirm","retryable":false}}"#,
    );
    let still_present: i64 = fixture
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM mailboxes WHERE id=?1",
            params![id],
            |row| row.get(0),
        )
        .expect("check mailbox after refused removal");
    assert_eq!(still_present, 1);

    let removed = fixture.skrzynka(&["mailbox", "remove", id, "--confirm"]);
    fixture.assert_success("remove confirmed mailbox", removed);
    let remaining: i64 = fixture
        .connection()
        .query_row("SELECT COUNT(*) FROM mailboxes", [], |row| row.get(0))
        .expect("count mailboxes after removal");
    assert_eq!(remaining, 0);

    let credential = fixture.skarbiec(&["get", "team-inbox", "--field", "password"]);
    assert_success("read preserved Skarbiec item", &credential);
    assert_eq!(
        String::from_utf8_lossy(&credential.stdout),
        format!("{PASSWORD}\n")
    );

    let missing = fixture.skrzynka(&["mailbox", "remove", id, "--confirm"]);
    assert_exit_one_with(
        &missing,
        r#"{"error":{"code":"NOT_FOUND","message":"mailbox was not found","retryable":false}}"#,
    );
}

#[test]
fn schema_three_migration_adds_smtp_credential_without_rewriting_mail_history() {
    let fixture = MailboxFixture::without_skarbiec("schema-three-migration");
    let connection = Connection::open(&fixture.database).expect("open schema-three database");
    connection
        .execute_batch(
            r#"
            PRAGMA foreign_keys=ON;
            CREATE TABLE mailboxes (
                id TEXT PRIMARY KEY,
                organization_id TEXT NOT NULL,
                skarbiec_item_id TEXT NOT NULL UNIQUE,
                display_name TEXT NOT NULL,
                email TEXT NOT NULL,
                imap_host TEXT NOT NULL,
                imap_port INTEGER NOT NULL,
                smtp_host TEXT NOT NULL,
                smtp_port INTEGER NOT NULL,
                smtp_security TEXT NOT NULL CHECK (smtp_security IN ('starttls', 'tls')),
                poll_interval_seconds INTEGER NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                last_uid INTEGER NOT NULL DEFAULT 0,
                last_sync_at TEXT,
                last_error_code TEXT,
                last_error_message TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE outbound_messages (
                id TEXT PRIMARY KEY,
                mailbox_id TEXT NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
                idempotency_key TEXT NOT NULL UNIQUE,
                recipients TEXT NOT NULL,
                cc TEXT,
                subject TEXT NOT NULL,
                body TEXT NOT NULL,
                status TEXT NOT NULL CHECK (status IN ('pending', 'sending', 'sent', 'failed', 'uncertain')),
                provider_message_id TEXT,
                error_code TEXT,
                error_message TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                sent_at TEXT
            );
            INSERT INTO mailboxes (
                id, organization_id, skarbiec_item_id, display_name, email,
                imap_host, imap_port, smtp_host, smtp_port, smtp_security,
                poll_interval_seconds, enabled, last_uid, created_at, updated_at
            ) VALUES (
                '00000000-0000-0000-0000-000000000010', 'legacy-local',
                'legacy-credential', 'Legacy mailbox', 'legacy@example.invalid',
                'imap.example.invalid', 993, 'smtp.example.invalid', 587,
                'starttls', 60, 0, 42, '2026-09-01T00:00:00Z',
                '2026-09-01T00:00:00Z'
            );
            INSERT INTO outbound_messages (
                id, mailbox_id, idempotency_key, recipients, subject, body,
                status, provider_message_id, created_at, updated_at, sent_at
            ) VALUES (
                '00000000-0000-0000-0000-000000000011',
                '00000000-0000-0000-0000-000000000010', 'legacy-send',
                'buyer@example.invalid', 'Legacy subject', 'Legacy body', 'sent',
                '<legacy@example.invalid>', '2026-09-01T00:00:00Z',
                '2026-09-01T00:00:01Z', '2026-09-01T00:00:01Z'
            );
            PRAGMA user_version=3;
            "#,
        )
        .expect("seed schema-three state");
    drop(connection);

    let listed = fixture.skrzynka(&["mailbox", "list"]);
    assert_success("open and migrate schema-three database", &listed);

    let connection = fixture.connection();
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read migrated schema version");
    assert_eq!(version, 4);
    let mailbox = connection
        .query_row(
            "SELECT skarbiec_item_id, smtp_skarbiec_item_id, email, smtp_host, enabled, last_uid FROM mailboxes",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        )
        .expect("read migrated mailbox");
    assert_eq!(
        mailbox,
        (
            "legacy-credential".to_string(),
            None,
            "legacy@example.invalid".to_string(),
            "smtp.example.invalid".to_string(),
            0,
            42,
        )
    );
    let outbound = connection
        .query_row(
            "SELECT idempotency_key, status, provider_message_id FROM outbound_messages",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .expect("read preserved outbound message");
    assert_eq!(
        outbound,
        (
            "legacy-send".to_string(),
            "sent".to_string(),
            Some("<legacy@example.invalid>".to_string()),
        )
    );
}

#[test]
fn without_a_home_directory_the_default_paths_are_refused_not_guessed() {
    let mut status = Command::new(env!("CARGO_BIN_EXE_skrzynka"));
    status
        .arg("status")
        .env_remove("HOME")
        .env_remove("XDG_STATE_HOME");
    let output = status.output().expect("run real Skrzynka binary");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("DATABASE_PATH_REQUIRED"), "{stderr}");
    assert!(stderr.contains("pass --database <PATH>"), "{stderr}");

    let mut onboarding = Command::new(env!("CARGO_BIN_EXE_skrzynka"));
    onboarding
        .arg("onboarding")
        .env_remove("HOME")
        .env_remove("XDG_STATE_HOME")
        .stdin(Stdio::null());
    let output = onboarding.output().expect("run real Skrzynka binary");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("neither XDG_STATE_HOME nor HOME is set"),
        "{stderr}"
    );
}
