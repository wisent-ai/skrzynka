use rusqlite::params;
use std::fs;

use super::fixture::*;
#[test]
fn mailbox_add_persists_only_the_profile_and_refuses_duplicate_or_invalid_accounts() {
    let fixture = MailboxFixture::new("add");
    fixture.seed_mailbox_item("team-inbox");
    fixture.seed_mailbox_item("invalid-inbox");

    let created = fixture.add_mailbox("team-inbox");
    let id = MailboxFixture::mailbox_id(&created);
    let stored = fixture
        .connection()
        .query_row(
            "SELECT skarbiec_item_id, display_name, email, imap_host, imap_port, smtp_host, smtp_port, smtp_security, poll_interval_seconds, enabled, last_uid FROM mailboxes WHERE id=?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, i64>(10)?,
                ))
            },
        )
        .expect("read persisted mailbox state");
    assert_eq!(
        stored,
        (
            "team-inbox".to_string(),
            "Team Inbox".to_string(),
            "team@example.invalid".to_string(),
            "imap.example.invalid".to_string(),
            993,
            "smtp.example.invalid".to_string(),
            587,
            "starttls".to_string(),
            60,
            1,
            0,
        )
    );
    let database_bytes = fs::read(&fixture.database).expect("read SQLite state");
    assert!(
        !database_bytes
            .windows(PASSWORD.len())
            .any(|window| window == PASSWORD.as_bytes()),
        "the mailbox password must never enter SQLite"
    );

    let duplicate = fixture.skrzynka(&[
        "mailbox",
        "add",
        "--skarbiec-item",
        "team-inbox",
    ]);
    assert_exit_one_with(
        &duplicate,
        r#"{"error":{"code":"MAILBOX_ALREADY_EXISTS","message":"a mailbox already uses this Skarbiec item","retryable":false}}"#,
    );

    let invalid = fixture.skrzynka(&[
        "mailbox",
        "add",
        "--skarbiec-item",
        "invalid-inbox",
        "--email",
        "not-an-address",
    ]);
    assert_exit_one_with(
        &invalid,
        r#"{"error":{"code":"MAILBOX_PROFILE_INVALID","message":"email is not a valid address","retryable":false}}"#,
    );
    let count: i64 = fixture
        .connection()
        .query_row("SELECT COUNT(*) FROM mailboxes", [], |row| row.get(0))
        .expect("count persisted mailboxes");
    assert_eq!(count, 1, "refused creates must not write mailbox state");
}

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
    assert_eq!(String::from_utf8_lossy(&credential.stdout), format!("{PASSWORD}\n"));

    let missing = fixture.skrzynka(&["mailbox", "remove", id, "--confirm"]);
    assert_exit_one_with(
        &missing,
        r#"{"error":{"code":"NOT_FOUND","message":"mailbox was not found","retryable":false}}"#,
    );
}
