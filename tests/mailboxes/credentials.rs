use rusqlite::Connection;
use serde_json::Value;

use super::fixture::*;
#[test]
fn gmail_app_password_mailbox_selector_refusals_leave_credentials_and_mailboxes_unchanged() {
    const GMAIL_EMAIL: &str = "selector-refusal@gmail.com";
    const GMAIL_ITEM: &str = "skrzynka-gmail-app-password-0d4931795b42e0dc4226";
    const SHARED_ADDRESS: &str = "team@example.invalid";

    let fixture = MailboxFixture::new("gmail-selector");
    fixture.seed_mailbox_item("alpha-inbox");
    fixture.seed_mailbox_item("beta-inbox");

    let alpha_output = fixture.skrzynka(&[
        "mailbox",
        "add",
        "--skarbiec-item",
        "alpha-inbox",
        "--display-name",
        "Alpha Inbox",
    ]);
    assert_success("add first same-address mailbox", &alpha_output);
    let alpha: Value =
        serde_json::from_slice(&alpha_output.stdout).expect("first mailbox add must return JSON");

    let beta_output = fixture.skrzynka(&[
        "mailbox",
        "add",
        "--skarbiec-item",
        "beta-inbox",
        "--display-name",
        "Beta Inbox",
    ]);
    assert_success("add second same-address mailbox", &beta_output);
    let beta: Value =
        serde_json::from_slice(&beta_output.stdout).expect("second mailbox add must return JSON");

    let baseline = mailbox_receiving_state(&fixture);
    assert_eq!(baseline.len(), 2, "fixture must contain exactly two mailboxes");
    fixture.assert_skarbiec_item_absent(GMAIL_ITEM);

    let missing = fixture.skrzynka_with_stdin(
        &[
            "gmail",
            "app-password",
            "--email",
            GMAIL_EMAIL,
            "--mailbox",
            UNKNOWN_MAILBOX,
        ],
        PASSWORD,
    );
    assert_exit_one_with(
        &missing,
        r#"{"error":{"code":"NOT_FOUND","message":"mailbox was not found","retryable":false}}"#,
    );
    assert_eq!(
        mailbox_receiving_state(&fixture),
        baseline,
        "unknown selector refusal must not modify or add mailbox rows"
    );
    fixture.assert_skarbiec_item_absent(GMAIL_ITEM);

    let ambiguous = fixture.skrzynka_with_stdin(
        &[
            "gmail",
            "app-password",
            "--email",
            GMAIL_EMAIL,
            "--mailbox",
            SHARED_ADDRESS,
        ],
        PASSWORD,
    );
    let ambiguous_error = format!(
        r#"{{"error":{{"code":"MAILBOX_SELECTOR_AMBIGUOUS","message":"{SHARED_ADDRESS} names 2 mailboxes ({}, {}); select one by id","retryable":false}}}}"#,
        MailboxFixture::mailbox_id(&alpha),
        MailboxFixture::mailbox_id(&beta)
    );
    assert_exit_one_with(&ambiguous, &ambiguous_error);
    assert_eq!(
        mailbox_receiving_state(&fixture),
        baseline,
        "ambiguous selector refusal must not modify or add mailbox rows"
    );
    fixture.assert_skarbiec_item_absent(GMAIL_ITEM);
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
